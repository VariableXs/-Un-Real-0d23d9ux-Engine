//! UNX-F10303 syscall 号面一致性核（表锚 SYS_NR_BASE 对平）（0x1b060 段）
//!
//! 锚点：UNX-F10303-J1 号面对平——本侧表逐行 nr 恰为表锚+行序，双表
//! 同序同名才判一致，面外号查询必拒。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：号面表（行号自动配发 nr=anchor+len，重名独立拒）+ 一致性核
//! （锚位逐行核+双表对平）+ 查表面（nr→名定向查找，越界拒绝）+
//! 注入核（乱序双表/重名/面外号/超容四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b060 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b060;
pub const NR_DUP: u32 = SEG_BASE + 0x01; // 行名重号
pub const NR_EMPTY: u32 = SEG_BASE + 0x02; // 行名空
pub const NR_CAP: u32 = SEG_BASE + 0x03; // 表超容
pub const NR_ANCHOR: u32 = SEG_BASE + 0x04; // 行号偏离表锚
pub const NR_OUTSIDE: u32 = SEG_BASE + 0x05; // 面外号
pub const NR_TWIN: u32 = SEG_BASE + 0x06; // 双表对平失败

/// 表锚 SYS_NR_BASE。
pub const SYS_NR_BASE: u32 = 0x3000;
/// 号面行数锚。
pub const ROWS_NEEDED: usize = 12;
/// 表定容。
pub const CAP: usize = 16;

/// 单行号面。
#[derive(Clone)]
pub struct NrRow {
    pub nr: u32,
    pub name: String,
}

impl NrRow {
    pub fn ok(&self, anchor: u32, seq: usize) -> bool {
        self.nr == anchor + seq as u32 && !self.name.is_empty()
    }
}

/// syscall 号面表。
pub struct NrTable {
    pub rows: Vec<NrRow>,
    pub anchor: u32,
    pub sealed: bool,
}

impl NrTable {
    pub fn new(anchor: u32) -> NrTable {
        NrTable { rows: Vec::new(), anchor, sealed: false }
    }

    /// 登记一行：号自动配发（anchor+行序）。封表、重名、空名、超容均拒。
    pub fn register(&mut self, name: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(NR_CAP);
        }
        if name.is_empty() {
            return Err(NR_EMPTY);
        }
        if self.rows.iter().any(|r| r.name == name) {
            return Err(NR_DUP);
        }
        if self.rows.len() >= CAP {
            return Err(NR_CAP);
        }
        let nr = self.anchor + self.rows.len() as u32;
        self.rows.push(NrRow { nr, name: name.to_string() });
        Ok(nr)
    }

    /// 锚位逐行核：每行 nr 恰为 anchor+行序。
    pub fn verify_plane(&self) -> bool {
        let mut k = 0usize;
        while k < self.rows.len() {
            if !self.rows[k].ok(self.anchor, k) {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 面内定向查找：nr→名。
    pub fn lookup(&self, nr: u32) -> Result<&str, u32> {
        if nr < self.anchor {
            return Err(NR_OUTSIDE);
        }
        let idx = (nr - self.anchor) as usize;
        if idx >= self.rows.len() {
            return Err(NR_OUTSIDE);
        }
        Ok(self.rows[idx].name.as_str())
    }

    /// 行序定向取名（越序报面外）。
    pub fn name_at(&self, seq: usize) -> Result<&str, u32> {
        if seq >= self.rows.len() {
            return Err(NR_OUTSIDE);
        }
        Ok(self.rows[seq].name.as_str())
    }

    /// 双表对平：行数相等且逐行同名（序敏感）。
    pub fn conforms(&self, peer: &NrTable) -> Result<(), u32> {
        if self.rows.len() != peer.rows.len() {
            return Err(NR_TWIN);
        }
        let mut k = 0usize;
        while k < self.rows.len() {
            if self.rows[k].name != peer.rows[k].name {
                return Err(NR_TWIN);
            }
            k += 1;
        }
        Ok(())
    }

    /// 双表行号面对平：逐行 nr 相等。
    pub fn nr_aligned(&self, peer: &NrTable) -> bool {
        if self.rows.len() != peer.rows.len() {
            return false;
        }
        let mut k = 0usize;
        while k < self.rows.len() {
            if self.rows[k].nr != peer.rows[k].nr {
                return false;
            }
            k += 1;
        }
        true
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 表折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b0600 ^ self.anchor;
        h = h.wrapping_mul(31).wrapping_add(self.rows.len() as u32);
        let mut k = 0usize;
        while k < self.rows.len() {
            h = h.wrapping_mul(31).wrapping_add(self.rows[k].nr);
            let bytes = self.rows[k].name.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = h.wrapping_mul(31).wrapping_add(bytes[b] as u32);
                b += 1;
            }
            k += 1;
        }
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 双表乱序（乙侧两行互换）必判对平失败
    let mut a = NrTable::new(SYS_NR_BASE);
    let _ = a.register("open");
    let _ = a.register("read");
    let mut b = NrTable::new(SYS_NR_BASE);
    let _ = b.register("read");
    let _ = b.register("open");
    got[0] = a.conforms(&b).err() == Some(NR_TWIN);
    // 2) 重名必拒
    let mut d = NrTable::new(SYS_NR_BASE);
    let _ = d.register("dup");
    got[1] = d.register("dup").err() == Some(NR_DUP);
    // 3) 面外号查询必拒
    got[2] = d.lookup(SYS_NR_BASE + 99).err() == Some(NR_OUTSIDE)
        && d.lookup(SYS_NR_BASE - 1).err() == Some(NR_OUTSIDE);
    // 4) 超容必拒
    let mut c = NrTable::new(SYS_NR_BASE);
    let mut n = 0usize;
    let mut full = true;
    while n < CAP {
        let nm = format!("fn{}", n);
        if c.register(&nm).is_err() {
            full = false;
        }
        n += 1;
    }
    got[3] = full && c.register("over").err() == Some(NR_CAP);
    got
}

/// canonical 十二行名册。
pub const CANON_NAMES: [&str; ROWS_NEEDED] = [
    "fs_open", "fs_close", "fs_read", "fs_write", "fs_lseek", "fs_stat",
    "fs_mkdir", "fs_rmdir", "fs_opendir", "fs_readdir", "fs_dup", "fs_pipe",
];

/// 按名册装配一号面表。
pub fn build_canon() -> NrTable {
    let mut t = NrTable::new(SYS_NR_BASE);
    let mut k = 0usize;
    while k < CANON_NAMES.len() {
        let _ = t.register(CANON_NAMES[k]);
        k += 1;
    }
    t
}

/// 批内判据。
pub fn run_unx10303_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10303");

    // J1 正路径：十二行装配且锚位逐行核过
    let t = build_canon();
    let j1 = t.rows.len() == ROWS_NEEDED && t.verify_plane();
    s.add("J1-号面锚位核", j1, "十二行 nr 恰为表锚+行序");

    // J2 行字段齐备
    let j2 = {
        let mut ok = true;
        let mut k = 0usize;
        while k < t.rows.len() {
            if t.rows[k].name.is_empty() || t.rows[k].nr < SYS_NR_BASE {
                ok = false;
            }
            k += 1;
        }
        ok
    };
    s.add("J2-行字段齐", j2, "行名非空且 nr 不低于表锚");

    // J3 双表对平：同册重建逐行同名同号
    let twin = build_canon();
    let j3 = t.conforms(&twin).is_ok() && t.nr_aligned(&twin);
    s.add("J3-双表对平", j3, "同册重建双表逐行一致");

    // J4 注入检出：乱序双表
    let inj = inject_four();
    s.add("J4-乱序检出", inj[0], "双表行序漂移必判对平失败");

    // J5 注入检出：重名 + 面外号
    s.add("J5-重名越界检出", inj[1] && inj[2], "重名必拒且面外号必拒");

    // J6 注入检出：超容
    s.add("J6-超容检出", inj[3], "表超容必拒且原表不损");

    // J7 定向查找：首行与末行均可命中
    let j7 = t.lookup(SYS_NR_BASE) == Ok("fs_open")
        && t.lookup(SYS_NR_BASE + (ROWS_NEEDED as u32 - 1)) == Ok("fs_pipe");
    s.add("J7-首末行查找", j7, "面内首末行定向查找命中");

    // J8 行号连续性：相邻行号差恰 1
    let j8 = {
        let mut ok = true;
        let mut k = 1usize;
        while k < t.rows.len() {
            if t.rows[k].nr != t.rows[k - 1].nr + 1 {
                ok = false;
            }
            k += 1;
        }
        ok
    };
    s.add("J8-行号连续", j8, "行号逐行递增差 1");

    // J9 负路径：空行名拒
    let j9 = NrTable::new(SYS_NR_BASE).register("").err() == Some(NR_EMPTY);
    s.add("J9-空名拒收", j9, "行名空必拒");

    // J10 表尾外一号报越界（恰在锚+len 处）
    let j10 = t.lookup(SYS_NR_BASE + ROWS_NEEDED as u32).err() == Some(NR_OUTSIDE);
    s.add("J10-表尾越界", j10, "行号恰越表尾必拒");

    // J11 封表后登记按超容面拒（号面终态不可再扩）
    let j11 = {
        let mut c = build_canon();
        c.seal();
        c.register("late").err() == Some(NR_CAP)
    };
    s.add("J11-封表拒扩", j11, "封表后登记必拒");

    // J12 动态明细：表锚与行数落账
    let fp = t.fold();
    s.add_dyn(
        "J12-表指纹明细",
        fp != 0 && t.rows.len() == ROWS_NEEDED,
        &format!("anchor={:#x} rows={}", t.anchor, t.rows.len()),
    );

    // J13 动态明细：首行号与末行号对账
    let lo = t.rows.first().map(|r| r.nr).unwrap_or(0);
    let hi = t.rows.last().map(|r| r.nr).unwrap_or(0);
    s.add_dyn(
        "J13-首末行号",
        hi == lo + (ROWS_NEEDED as u32 - 1),
        &format!("lo={:#x} hi={:#x}", lo, hi),
    );

    // J14 动态明细：行序定向取名对账
    let first = t.name_at(0);
    let last = t.name_at(ROWS_NEEDED - 1);
    let j14 = first == Ok("fs_open")
        && last == Ok("fs_pipe")
        && t.name_at(ROWS_NEEDED).err() == Some(NR_OUTSIDE);
    s.add_dyn(
        "J14-行序取名",
        j14,
        &format!("首行 {:?} 末行 {:?}", first, last),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10303_table_flow() {
        let mut t = NrTable::new(SYS_NR_BASE);
        assert_eq!(t.register("open"), Ok(SYS_NR_BASE));
        assert_eq!(t.register("read"), Ok(SYS_NR_BASE + 1));
        assert!(t.register("open").err() == Some(NR_DUP));
        assert!(t.register("").err() == Some(NR_EMPTY));
        assert!(t.verify_plane());
        assert_eq!(t.lookup(SYS_NR_BASE + 1), Ok("read"));
        assert!(t.lookup(SYS_NR_BASE + 2).err() == Some(NR_OUTSIDE));
        let mut p = NrTable::new(SYS_NR_BASE);
        let _ = p.register("read");
        let _ = p.register("open");
        assert!(t.conforms(&p).err() == Some(NR_TWIN));
        t.seal();
        assert!(t.register("late").err() == Some(NR_CAP));
    }

    #[test]
    fn unx10303_checks_green() {
        let s = run_unx10303_checks();
        assert!(s.all_passed(), "UNX-F10303 判据未全绿");
    }
}
