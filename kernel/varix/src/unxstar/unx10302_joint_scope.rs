//! UNX-F10302 联签范围清单（fs 域对接号面枚举）（0x1b040 段）
//!
//! 锚点：UNX-F10302-J1 号面枚举完备——fs 域对接号面区间两两不相交、
//! 计数对平且覆盖锚值，封单后不可再扩。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：范围册（对接号面按 [base, base+count) 连续段登记，重叠检测
//! 独立成路）+ 覆盖核（号点归属查询）+ 封单核（封单后扩单即拒）+
//! 注入核（重叠/零长/越界号点/封单后扩单四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b040 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b040;
pub const SC_OVERLAP: u32 = SEG_BASE + 0x01; // 号面区间重叠
pub const SC_ZERO: u32 = SEG_BASE + 0x02; // 零长号面
pub const SC_EMPTY: u32 = SEG_BASE + 0x03; // 段标空
pub const SC_CAP: u32 = SEG_BASE + 0x04; // 超容
pub const SC_SEALED: u32 = SEG_BASE + 0x05; // 封单后扩单
pub const SC_OUTSIDE: u32 = SEG_BASE + 0x06; // 号点不在枚举面内

/// fs 域对接号面基锚。
pub const FS_FACE_BASE: u32 = 0x3000;
/// 枚举面总覆盖锚（六段合计）。
pub const FACE_TOTAL: u64 = 68;
/// 范围册定容。
pub const CAP: usize = 12;

/// 单条对接号面段：[base, base+count)。
#[derive(Clone)]
pub struct FaceRange {
    pub base: u32,
    pub count: u32,
    pub tag: String,
}

impl FaceRange {
    /// 段尾（开区间端）。
    pub fn end(&self) -> u32 {
        self.base.wrapping_add(self.count)
    }

    /// 号点归属。
    pub fn holds(&self, nr: u32) -> bool {
        nr >= self.base && nr < self.end()
    }

    pub fn ok(&self) -> bool {
        self.count > 0 && !self.tag.is_empty()
    }

    pub fn render(&self) -> String {
        format!("F[{:x}..{:x})|{}", self.base, self.end(), self.tag)
    }
}

/// 联签范围清单册。
pub struct ScopeList {
    pub ranges: Vec<FaceRange>,
    pub sealed: bool,
}

impl ScopeList {
    pub fn new() -> ScopeList {
        ScopeList { ranges: Vec::new(), sealed: false }
    }

    /// 段重叠检测：与任一既有段相交即拒。
    fn overlaps(&self, base: u32, count: u32) -> bool {
        let end = base.wrapping_add(count);
        self.ranges
            .iter()
            .any(|r| base < r.end() && r.base < end)
    }

    /// 登记号面段：封单、零长、空标、重叠、超容均拒。
    pub fn add(&mut self, base: u32, count: u32, tag: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(SC_SEALED);
        }
        if count == 0 {
            return Err(SC_ZERO);
        }
        if tag.is_empty() {
            return Err(SC_EMPTY);
        }
        if self.overlaps(base, count) {
            return Err(SC_OVERLAP);
        }
        if self.ranges.len() >= CAP {
            return Err(SC_CAP);
        }
        self.ranges.push(FaceRange { base, count, tag: tag.to_string() });
        Ok(())
    }

    /// 号点归属查询：命中返回 Ok(段序)，越界返回 SC_OUTSIDE。
    pub fn locate(&self, nr: u32) -> Result<usize, u32> {
        let mut k = 0usize;
        while k < self.ranges.len() {
            if self.ranges[k].holds(nr) {
                return Ok(k);
            }
            k += 1;
        }
        Err(SC_OUTSIDE)
    }

    /// 段标清单（段序敏感）。
    pub fn tags(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut k = 0usize;
        while k < self.ranges.len() {
            out.push(self.ranges[k].tag.clone());
            k += 1;
        }
        out
    }

    /// 号点 → 所在段渲染（未命中即空串）。
    pub fn render_at(&self, nr: u32) -> String {
        match self.locate(nr) {
            Ok(k) => self.ranges[k].render(),
            Err(_) => String::new(),
        }
    }

    /// 覆盖总数（合计号点数）。
    pub fn total(&self) -> u64 {
        let mut t: u64 = 0;
        let mut k = 0usize;
        while k < self.ranges.len() {
            t = t.wrapping_add(self.ranges[k].count as u64);
            k += 1;
        }
        t
    }

    /// 两两不相交复核（登记时已拒，此处为对账复核）。
    pub fn disjoint(&self) -> bool {
        let mut i = 0usize;
        while i < self.ranges.len() {
            let mut j = i + 1;
            while j < self.ranges.len() {
                let a = &self.ranges[i];
                let b = &self.ranges[j];
                if a.base < b.end() && b.base < a.end() {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    /// 枚举完备：不相交、计数对平锚、段标全合法。
    pub fn complete(&self) -> bool {
        self.disjoint() && self.total() == FACE_TOTAL && self.ranges.iter().all(|r| r.ok())
    }

    /// 封单。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 清单折叠指纹（段序敏感）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b0400;
        h = h.wrapping_mul(31).wrapping_add(self.ranges.len() as u32);
        let mut k = 0usize;
        while k < self.ranges.len() {
            let r = &self.ranges[k];
            h = h.wrapping_mul(31).wrapping_add(r.base);
            h = h.wrapping_mul(31).wrapping_add(r.count);
            let bytes = r.tag.as_bytes();
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
    // 1) 区间重叠必拒
    let mut l = ScopeList::new();
    let _ = l.add(FS_FACE_BASE, 8, "open 族");
    got[0] = l.add(FS_FACE_BASE + 4, 4, "重叠段").err() == Some(SC_OVERLAP);
    // 2) 零长号面必拒
    got[1] = l.add(FS_FACE_BASE + 100, 0, "零长").err() == Some(SC_ZERO);
    // 3) 枚举面外号点必报越界
    got[2] = l.locate(FS_FACE_BASE + 8).err() == Some(SC_OUTSIDE);
    // 4) 封单后扩单必抓
    l.seal();
    got[3] = l.add(FS_FACE_BASE + 50, 4, "迟到段").err() == Some(SC_SEALED);
    got
}

/// 六段 canonical 样本（合计恰为 FACE_TOTAL）。
pub fn canonical_scopes() -> [(u32, u32, &'static str); 6] {
    [
        (FS_FACE_BASE, 8, "open 族"),
        (FS_FACE_BASE + 8, 16, "read 族"),
        (FS_FACE_BASE + 24, 16, "write 族"),
        (FS_FACE_BASE + 40, 8, "stat 族"),
        (FS_FACE_BASE + 48, 8, "dir 族"),
        (FS_FACE_BASE + 56, 12, "path 族"),
    ]
}

/// 批内判据。
pub fn run_unx10302_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10302");

    // J1 正路径：六段 canonical 样本登记并完备
    let mut list = ScopeList::new();
    let canon = canonical_scopes();
    let mut k = 0usize;
    let mut all_ok = true;
    while k < canon.len() {
        if list.add(canon[k].0, canon[k].1, canon[k].2).is_err() {
            all_ok = false;
        }
        k += 1;
    }
    s.add("J1-六段枚举完备", all_ok && list.complete(), "六段登记且计数对平 68");

    // J2 段标全合法
    let j2 = list.ranges.iter().all(|r| r.ok());
    s.add("J2-段标合法", j2, "段长非零且标非空");

    // J3 号点归属：各段首点可定位
    let j3 = {
        let mut ok = true;
        let mut n = 0usize;
        while n < canon.len() {
            if list.locate(canon[n].0).err() == Some(SC_OUTSIDE) {
                ok = false;
            }
            n += 1;
        }
        ok
    };
    s.add("J3-段首可定位", j3, "六段首点均落在枚举面内");

    // J4 注入检出：重叠 + 零长
    let inj = inject_four();
    s.add("J4-重叠零长检出", inj[0] && inj[1], "重叠必拒且零长必拒");

    // J5 注入检出：越界 + 封单后扩单
    s.add("J5-越界封单检出", inj[2] && inj[3], "面外号点必报且封单后必拒");

    // J6 相邻段可拼接：紧邻段不判重叠
    let j6 = {
        let mut c = ScopeList::new();
        c.add(0x10, 4, "甲").is_ok() && c.add(0x14, 4, "乙").is_ok()
    };
    s.add("J6-紧邻合法", j6, "端点相接不算重叠");

    // J7 容量边界：第 CAP+1 段拒
    let j7 = {
        let mut c = ScopeList::new();
        let mut filled = true;
        let mut n = 0u32;
        while n < CAP as u32 {
            if c.add(n * 16, 8, "x").is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.add(9999, 4, "over").err() == Some(SC_CAP)
    };
    s.add("J7-容量边界", j7, "超容必拒且不破坏原单");

    // J8 空段标拒
    let j8 = ScopeList::new().add(0x20, 4, "").err() == Some(SC_EMPTY);
    s.add("J8-空标拒收", j8, "段标空必拒");

    // J9 计数对平：total 恰等于锚
    let j9 = list.total() == FACE_TOTAL;
    s.add("J9-计数对平", j9, "合计号点数 68 对平");

    // J10 封单指纹稳定
    let fp0 = list.fold();
    let fp1 = list.seal();
    let j10 = fp0 == fp1 && list.sealed;
    s.add("J10-封单指纹", j10, "封单前后指纹一致且封单位翻转");

    // J11 负路径：跨在两段之间的号点报越界（不误命中）
    let j11 = {
        let mut c = ScopeList::new();
        let _ = c.add(0x40, 4, "甲");
        let _ = c.add(0x50, 4, "乙");
        c.locate(0x48).err() == Some(SC_OUTSIDE)
    };
    s.add("J11-段间越界", j11, "两段间空档号点不误命中");

    // J12 动态明细：总覆盖数落账
    let t = list.total();
    s.add_dyn("J12-覆盖总数", t == FACE_TOTAL, &format!("total={}", t));

    // J13 动态明细：段数明细
    let n = list.ranges.len();
    s.add_dyn("J13-段数明细", n == 6, &format!("ranges={}", n));

    // J14 动态明细：段标清单与号点渲染
    let tags = list.tags();
    let j14 = tags.len() == 6
        && tags[0] == "open 族"
        && !list.render_at(FS_FACE_BASE).is_empty()
        && list.render_at(FS_FACE_BASE + 68).is_empty();
    s.add_dyn("J14-段标渲染", j14, &format!("段标 {:?}", tags));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10302_scope_flow() {
        let mut l = ScopeList::new();
        assert!(l.add(0x100, 8, "a").is_ok());
        assert!(l.add(0x104, 2, "overlap").err() == Some(SC_OVERLAP));
        assert!(l.add(0x110, 0, "z").err() == Some(SC_ZERO));
        assert!(l.add(0x110, 4, "").err() == Some(SC_EMPTY));
        assert_eq!(l.locate(0x100), Ok(0));
        assert!(l.locate(0x108).err() == Some(SC_OUTSIDE));
        assert!(l.disjoint());
        l.seal();
        assert!(l.add(0x200, 4, "late").err() == Some(SC_SEALED));
    }

    #[test]
    fn unx10302_checks_green() {
        let s = run_unx10302_checks();
        assert!(s.all_passed(), "UNX-F10302 判据未全绿");
    }
}
