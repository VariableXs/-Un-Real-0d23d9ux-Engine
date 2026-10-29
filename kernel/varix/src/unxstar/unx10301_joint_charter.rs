//! UNX-F10301 联签开卷总纲（联签对象/判据/口径三维齐备）（0x1b020 段）
//!
//! 锚点：UNX-F10301-J1 三维齐备——联签对象面/判据面/口径面三册各达锚数
//! 且零空名，方可开卷。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：三面册（对象册/判据册/口径册各自定容登记，重号与空名独立拒）+
//! 开卷核（三维锚数齐备才许开卷，开卷后写册即拒）+ 注入核（缺维/重号/
//! 空名/开卷后写册四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b020 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b020;
pub const JC_DUP: u32 = SEG_BASE + 0x01; // 维内重号
pub const JC_EMPTY: u32 = SEG_BASE + 0x02; // 条目空名
pub const JC_CAP: u32 = SEG_BASE + 0x03; // 维内超容
pub const JC_SHORT: u32 = SEG_BASE + 0x04; // 维内缺额（不可开卷）
pub const JC_OPENED: u32 = SEG_BASE + 0x05; // 开卷后写册
pub const JC_BADDIM: u32 = SEG_BASE + 0x06; // 维号非法

/// 维号：1=联签对象 2=判据 3=口径。
pub const DIM_OBJECT: u8 = 1;
pub const DIM_CRITERIA: u8 = 2;
pub const DIM_GAUGE: u8 = 3;

/// 各维锚数（对象 3 / 判据 4 / 口径 2）。
pub const NEED_OBJECT: usize = 3;
pub const NEED_CRITERIA: usize = 4;
pub const NEED_GAUGE: usize = 2;
/// 单维定容。
pub const CAP: usize = 8;

/// 单条册目（三维共用形，按维号分册）。
#[derive(Clone)]
pub struct CharterRow {
    pub dim: u8,
    pub key: u32,
    pub name: String,
}

impl CharterRow {
    pub fn ok(&self) -> bool {
        self.key != 0 && !self.name.is_empty()
    }
}

/// 联签开卷总纲册。
pub struct JointCharter {
    pub rows: Vec<CharterRow>,
    pub opened: bool,
    pub note: String,
}

impl JointCharter {
    pub fn new(note: &str) -> JointCharter {
        JointCharter { rows: Vec::new(), opened: false, note: note.to_string() }
    }

    /// 维内条数。
    pub fn dim_count(&self, dim: u8) -> usize {
        self.rows.iter().filter(|r| r.dim == dim).count()
    }

    /// 维内键重号检测。
    pub fn dim_has(&self, dim: u8, key: u32) -> bool {
        self.rows.iter().any(|r| r.dim == dim && r.key == key)
    }

    /// 维号合法性。
    pub fn dim_ok(dim: u8) -> bool {
        dim == DIM_OBJECT || dim == DIM_CRITERIA || dim == DIM_GAUGE
    }

    /// 登记册目：非法维、重号、空名、超容、开卷后均拒。
    pub fn add(&mut self, dim: u8, key: u32, name: &str) -> Result<(), u32> {
        if self.opened {
            return Err(JC_OPENED);
        }
        if !JointCharter::dim_ok(dim) {
            return Err(JC_BADDIM);
        }
        if name.is_empty() {
            return Err(JC_EMPTY);
        }
        if key == 0 || self.dim_has(dim, key) {
            return Err(JC_DUP);
        }
        if self.dim_count(dim) >= CAP {
            return Err(JC_CAP);
        }
        self.rows.push(CharterRow { dim, key, name: name.to_string() });
        Ok(())
    }

    /// 单维齐备：达锚数且全部条目合法。
    pub fn dim_ready(&self, dim: u8, need: usize) -> bool {
        self.dim_count(dim) >= need
            && self.rows.iter().filter(|r| r.dim == dim).all(|r| r.ok())
    }

    /// 三维齐备裁断。
    pub fn three_ready(&self) -> bool {
        self.dim_ready(DIM_OBJECT, NEED_OBJECT)
            && self.dim_ready(DIM_CRITERIA, NEED_CRITERIA)
            && self.dim_ready(DIM_GAUGE, NEED_GAUGE)
    }

    /// 开卷：三维齐备才许开，返回总纲指纹。
    pub fn open(&mut self) -> Result<u32, u32> {
        if self.opened {
            return Err(JC_OPENED);
        }
        if !self.three_ready() {
            return Err(JC_SHORT);
        }
        self.opened = true;
        Ok(self.fold())
    }

    /// 总纲折叠指纹（维号→键→名字节序敏感）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b0200;
        h = h.wrapping_mul(31).wrapping_add(self.rows.len() as u32);
        let mut k = 0usize;
        while k < self.rows.len() {
            let r = &self.rows[k];
            h = h.wrapping_mul(31).wrapping_add(r.dim as u32);
            h = h.wrapping_mul(31).wrapping_add(r.key);
            let bytes = r.name.as_bytes();
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
    // 1) 缺维（口径维漏登）必开不了卷
    let mut c = JointCharter::new("inj-short");
    let mut n = 1u32;
    while n <= NEED_OBJECT as u32 {
        let _ = c.add(DIM_OBJECT, n, "对象");
        n += 1;
    }
    n = 1u32;
    while n <= NEED_CRITERIA as u32 {
        let _ = c.add(DIM_CRITERIA, n, "判据");
        n += 1;
    }
    got[0] = c.open().err() == Some(JC_SHORT);
    // 2) 维内重号必拒
    let mut d = JointCharter::new("inj-dup");
    let _ = d.add(DIM_OBJECT, 1, "甲");
    got[1] = d.add(DIM_OBJECT, 1, "乙").err() == Some(JC_DUP);
    // 3) 空名必拒
    got[2] = d.add(DIM_OBJECT, 2, "").err() == Some(JC_EMPTY);
    // 4) 开卷后写册必抓
    let mut f = JointCharter::new("inj-open");
    n = 1u32;
    while n <= NEED_OBJECT as u32 {
        let _ = f.add(DIM_OBJECT, n, "对象");
        n += 1;
    }
    n = 1u32;
    while n <= NEED_CRITERIA as u32 {
        let _ = f.add(DIM_CRITERIA, n, "判据");
        n += 1;
    }
    n = 1u32;
    while n <= NEED_GAUGE as u32 {
        let _ = f.add(DIM_GAUGE, n, "口径");
        n += 1;
    }
    let _ = f.open();
    got[3] = f.add(DIM_OBJECT, 9, "迟到").err() == Some(JC_OPENED);
    got
}

/// 批内判据。
pub fn run_unx10301_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10301");

    // J1 正路径：三维逐维登记并开卷
    let mut ch = JointCharter::new("F10301 开卷");
    let mut n = 1u32;
    while n <= NEED_OBJECT as u32 {
        let _ = ch.add(DIM_OBJECT, n, "联签对象甲乙丙");
        n += 1;
    }
    n = 1u32;
    while n <= NEED_CRITERIA as u32 {
        let _ = ch.add(DIM_CRITERIA, n, "联签判据条");
        n += 1;
    }
    n = 1u32;
    while n <= NEED_GAUGE as u32 {
        let _ = ch.add(DIM_GAUGE, n, "对平口径");
        n += 1;
    }
    let j1 = ch.three_ready() && ch.open().is_ok();
    s.add("J1-三维齐备开卷", j1, "对象/判据/口径三册达锚并开卷");

    // J2 条目字段齐备
    let j2 = ch.rows.iter().all(|r| r.ok());
    s.add("J2-条目字段齐", j2, "键非零且名非空");

    // J3 维号非法拒
    let j3 = JointCharter::new("bad").add(7, 1, "x").err() == Some(JC_BADDIM);
    s.add("J3-维号非法拒", j3, "三维之外的维号必拒");

    // J4 注入检出：缺维 + 重号
    let inj = inject_four();
    s.add("J4-缺维重号检出", inj[0] && inj[1], "缺维开不了卷且重号必拒");

    // J5 注入检出：空名 + 开卷后写
    s.add("J5-空名开卷检出", inj[2] && inj[3], "空名必拒且开卷后写册必抓");

    // J6 容量边界：单维第 CAP+1 条拒
    let j6 = {
        let mut c = JointCharter::new("cap");
        let mut filled = true;
        let mut k = 1u32;
        while k <= CAP as u32 {
            if c.add(DIM_CRITERIA, k, "x").is_err() {
                filled = false;
            }
            k += 1;
        }
        filled && c.add(DIM_CRITERIA, 99, "over").err() == Some(JC_CAP)
    };
    s.add("J6-容量边界", j6, "单维超容必拒");

    // J7 缺维裁断：判据维缺一条即三维不齐
    let j7 = {
        let mut c = JointCharter::new("short");
        let mut k = 1u32;
        while k <= NEED_OBJECT as u32 {
            let _ = c.add(DIM_OBJECT, k, "对象");
            k += 1;
        }
        let mut m = 1u32;
        while m < NEED_CRITERIA as u32 {
            let _ = c.add(DIM_CRITERIA, m, "判据");
            m += 1;
        }
        let mut p = 1u32;
        while p <= NEED_GAUGE as u32 {
            let _ = c.add(DIM_GAUGE, p, "口径");
            p += 1;
        }
        !c.three_ready() && c.open().err() == Some(JC_SHORT)
    };
    s.add("J7-缺一不开卷", j7, "判据维缺额即拒开卷");

    // J8 二次开卷拒
    let j8 = ch.open().err() == Some(JC_OPENED);
    s.add("J8-二次开卷拒", j8, "已开卷再开必拒");

    // J9 指纹稳定：两次 fold 一致
    let j9 = ch.fold() == ch.fold() && ch.fold() != 0;
    s.add("J9-指纹稳定", j9, "总纲折叠指纹两次一致");

    // J10 同键异维不冲突：对象/判据两维同键可共存
    let j10 = {
        let mut c = JointCharter::new("cross");
        c.add(DIM_OBJECT, 1, "对象一").is_ok() && c.add(DIM_CRITERIA, 1, "判据一").is_ok()
    };
    s.add("J10-跨维同键", j10, "重号只在维内判，跨维同键合法");

    // J11 负路径：零键拒
    let j11 = JointCharter::new("zero").add(DIM_GAUGE, 0, "零键").err() == Some(JC_DUP);
    s.add("J11-零键拒收", j11, "键为零按重号面拒");

    // J12 动态明细：开卷指纹落账
    let fp = ch.fold();
    s.add_dyn("J12-总纲指纹", fp != 0, &format!("fold={:#x}", fp));

    // J13 动态明细：三维计数明细
    let co = ch.dim_count(DIM_OBJECT);
    let cc = ch.dim_count(DIM_CRITERIA);
    let cg = ch.dim_count(DIM_GAUGE);
    s.add_dyn(
        "J13-维计数明细",
        co == NEED_OBJECT && cc == NEED_CRITERIA && cg == NEED_GAUGE,
        &format!("obj={} crit={} gauge={}", co, cc, cg),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10301_charter_flow() {
        let mut c = JointCharter::new("t");
        assert!(c.add(DIM_OBJECT, 1, "甲").is_ok());
        assert!(c.add(DIM_OBJECT, 1, "乙").err() == Some(JC_DUP));
        assert!(c.add(9, 2, "维").err() == Some(JC_BADDIM));
        assert!(!c.three_ready());
        assert!(c.open().err() == Some(JC_SHORT));
        let mut n = 2u32;
        while n <= NEED_OBJECT as u32 {
            let _ = c.add(DIM_OBJECT, n, "对象");
            n += 1;
        }
        n = 1u32;
        while n <= NEED_CRITERIA as u32 {
            let _ = c.add(DIM_CRITERIA, n, "判据");
            n += 1;
        }
        n = 1u32;
        while n <= NEED_GAUGE as u32 {
            let _ = c.add(DIM_GAUGE, n, "口径");
            n += 1;
        }
        assert!(c.open().is_ok());
        assert!(c.add(DIM_OBJECT, 9, "late").err() == Some(JC_OPENED));
    }

    #[test]
    fn unx10301_checks_green() {
        let s = run_unx10301_checks();
        assert!(s.all_passed(), "UNX-F10301 判据未全绿");
    }
}
