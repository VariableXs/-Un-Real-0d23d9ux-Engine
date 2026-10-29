//! UNX-F12117 UPCASE 表接口与不敏感哈希挂点（0x1a0c0 段）
//!
//! 卡面：UPCASE 表接口——表装载门、单元越域拒（> 0xFFFF）、
//! 不敏感哈希（先折叠后卷积）、同输入哈希稳定、哈希账重号拒。
//! 域：CoRun·UNX-D1-B06。
//!
//! 判据原文：
//! - J1 表装载门：UPCASE 表未装载时哈希必拒。
//! - J2 单元越域拒：单元 > 0xFFFF 必拒。
//! - J3 折叠口径：a-z 升 A-Z 后参与卷积，非字母恒等。
//! - J4 哈希稳定：同输入双算同值（幂等）。
//! - J5 折叠敏感性：大小写异形同哈希、异形敏感哈希不同。
//! - J6 哈希账重号拒：同源号二次登记必拒（含零号）。
//! - J7 印账冻结：seal 后操作必拒。
//! - J8 注入五况全抓。
//!
//! 结构：UpcaseTbl 表态 + hash_units 不敏感哈希核 + HashRec 哈希档 +
//! HashBook 哈希账 + 折叠指纹（211 进制卷积）+
//! 注入核（表未载/越域/零号/重号/印后操作）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1a0c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1a0c0;
pub const UH_NO_TABLE: u32 = SEG_BASE + 0x01; // 表未装载
pub const UH_OOB: u32 = SEG_BASE + 0x02; // 单元越域
pub const UH_DUP_ID: u32 = SEG_BASE + 0x03; // 重号（含零号）
pub const UH_FROZEN: u32 = SEG_BASE + 0x04; // 印账后操作

/// 单元折叠：a-z 升 A-Z。
pub fn fold_unit(u: u16) -> u16 {
    if u >= 0x61 && u <= 0x7A {
        u - 0x20
    } else {
        u
    }
}

/// UPCASE 表态。
pub struct UpcaseTbl {
    pub loaded: bool,
    pub stamp: u32,
}

impl UpcaseTbl {
    pub fn new() -> UpcaseTbl {
        UpcaseTbl { loaded: false, stamp: 0 }
    }
    /// 装载：表征指纹定格。
    pub fn load(&mut self) {
        let mut h: u32 = 0xCAFE_BABE;
        for u in (0u16..=0xFFu16).rev() {
            h = h.wrapping_mul(211).wrapping_add(fold_unit(u) as u32);
        }
        self.loaded = true;
        self.stamp = h;
    }
}

/// 不敏感哈希核：先折叠后 211 卷积。单元以 u32 表达（UPCASE 单元域为 u16，
/// > 0xFFFF 越域必拒——UH_OOB 面）。
pub fn hash_units(units: &[u32]) -> Result<u32, u32> {
    let mut h: u32 = SEG_BASE;
    for u in units {
        if *u > 0xFFFF {
            return Err(UH_OOB);
        }
        h = h.wrapping_mul(211).wrapping_add(fold_unit(*u as u16) as u32);
    }
    Ok(h)
}

/// 哈希档。
#[derive(Clone)]
pub struct HashRec {
    pub id: u32,
    pub units: Vec<u16>,
    pub hash: u32,
    pub tag: String,
}

impl HashRec {
    pub fn render(&self) -> String {
        format!("H{}|{:08x}", self.id, self.hash)
    }
}

/// 哈希账。
pub struct HashBook {
    pub table: UpcaseTbl,
    pub recs: Vec<HashRec>,
    pub sealed: bool,
}

impl HashBook {
    pub fn new() -> HashBook {
        HashBook { table: UpcaseTbl::new(), recs: Vec::new(), sealed: false }
    }

    /// 装载表。
    pub fn load_table(&mut self) {
        self.table.load();
    }

    /// 哈希登记：表装载 → 越域 → 重号 → 入账。
    pub fn put(&mut self, id: u32, units: &[u16], tag: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(UH_FROZEN);
        }
        if !self.table.loaded {
            return Err(UH_NO_TABLE);
        }
        let widened: Vec<u32> = units.iter().map(|&x| x as u32).collect();
        let h = hash_units(&widened)?;
        if id == 0 || self.recs.iter().any(|r| r.id == id) {
            return Err(UH_DUP_ID);
        }
        self.recs.push(HashRec {
            id,
            units: units.to_vec(),
            hash: h,
            tag: tag.to_string(),
        });
        Ok(h)
    }

    /// 哈希回读。
    pub fn hash_of(&self, id: u32) -> Option<u32> {
        self.recs.iter().find(|r| r.id == id).map(|r| r.hash)
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        h = h.wrapping_mul(211).wrapping_add(self.table.loaded as u32);
        h = h.wrapping_mul(211).wrapping_add(self.table.stamp);
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.id);
            h = h.wrapping_mul(211).wrapping_add(r.hash);
            for by in r.tag.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("recs={} loaded={}", self.recs.len(), self.table.loaded)
    }

    /// 诊断面：渲染行。
    pub fn line_of(&self, id: u32) -> String {
        match self.recs.iter().find(|r| r.id == id) {
            Some(r) => r.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = HashBook::new();
    // 1) 表未装载必拒。
    got[0] = b.put(1, &[0x41], "a").err() == Some(UH_NO_TABLE);
    // 2) 单元越域必拒（模型内 > 0xFFFF 不可表——以空表间接验证 OOB 面，
    //    越域面在 hash_units 直接测）。
    b.load_table();
    got[1] = hash_units(&[0x1_0000u32]).err() == Some(UH_OOB);
    // 3) 零号必拒。
    got[2] = b.put(0, &[0x41], "z").err() == Some(UH_DUP_ID);
    // 4) 重号必拒。
    let _ = b.put(2, &[0x41], "a");
    got[3] = b.put(2, &[0x42], "b").err() == Some(UH_DUP_ID);
    // 5) 印账后操作必抓。
    b.seal();
    got[4] = b.put(3, &[0x41], "p").err() == Some(UH_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12117_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12117");

    // J1 表装载门。
    let j1 = {
        let mut b = HashBook::new();
        b.put(1, &[0x41], "a").err() == Some(UH_NO_TABLE)
    };
    s.add("J1-表装载门", j1, "未装载必拒");

    // J2 单元越域拒。
    let j2 = hash_units(&[0x41u32, 0x1_0000]).err() == Some(UH_OOB);
    s.add("J2-越域拒", j2, "越 0xFFFF 必拒");

    // J3 折叠口径。
    let j3 = fold_unit(0x61) == 0x41
        && fold_unit(0x7A) == 0x5A
        && fold_unit(0x20) == 0x20
        && fold_unit(0x4E2D) == 0x4E2D;
    s.add("J3-折叠口径", j3, "小写升、其余恒等");

    // J4 哈希稳定（幂等）。
    let j4 = {
        let u = [0x64u32, 0x65, 0x76];
        hash_units(&u) == hash_units(&u)
    };
    s.add("J4-哈希稳定", j4, "同输入同值");

    // J5 折叠敏感性：异形同哈希。
    let j5 = {
        let lo = [0x64u32, 0x65];
        let up = [0x44u32, 0x45];
        hash_units(&lo) == hash_units(&up)
    };
    s.add("J5-异形同哈希", j5, "大小写折叠同键");

    // J6 敏感序哈希异。
    let j6 = {
        let a = [0x41u32, 0x42];
        let b = [0x41u32, 0x43];
        hash_units(&a) != hash_units(&b)
    };
    s.add("J6-异串异哈希", j6, "异元哈希必异");

    // J7 装载后登记与回读。
    let j7 = {
        let mut b = HashBook::new();
        b.load_table();
        let h = b.put(1, &[0x64, 0x65, 0x76], "dev").unwrap_or(0);
        b.hash_of(1) == Some(h) && h != 0
    };
    s.add("J7-登记回读", j7, "哈希可回读");

    // J8 重号拒。
    let j8 = {
        let mut b = HashBook::new();
        b.load_table();
        let _ = b.put(3, &[0x41], "a");
        b.put(3, &[0x42], "b").err() == Some(UH_DUP_ID)
    };
    s.add("J8-重号拒", j8, "同号二次必拒");

    // J9 零号拒。
    let j9 = {
        let mut b = HashBook::new();
        b.load_table();
        b.put(0, &[0x41], "z").err() == Some(UH_DUP_ID)
    };
    s.add("J9-零号拒", j9, "零号必拒");

    // J10 渲染行。
    let j10 = {
        let mut b = HashBook::new();
        b.load_table();
        let _ = b.put(5, &[0x41], "a");
        b.line_of(5).contains("H5") && b.line_of(6) == "missing"
    };
    s.add("J10-渲染行", j10, "哈希行可渲染");

    // J11 印账冻结。
    let j11 = {
        let mut b = HashBook::new();
        let _ = b.put(1, &[0x41], "a").is_err(); // 未装载先拒
        b.load_table();
        let _ = b.put(1, &[0x41], "a");
        b.seal();
        b.put(2, &[0x42], "p").err() == Some(UH_FROZEN)
    };
    s.add("J11-印账冻结", j11, "seal 后必拒");

    // J12 指纹幂等。
    let j12 = {
        let mut b = HashBook::new();
        b.load_table();
        let _ = b.put(1, &[0x41], "a");
        b.fold() == b.fold()
    };
    s.add("J12-指纹幂等", j12, "同态同指纹");

    // J13 注入核 5/5。
    let got = inject_five();
    let j13 = got.iter().all(|x| *x);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = HashBook::new();
    b.load_table();
    let h1 = b.put(1, &[0x64, 0x65, 0x76], "dev").unwrap_or(0);
    let h2 = b.put(2, &[0x44, 0x45, 0x56], "DEV").unwrap_or(0);

    s.add("摘要回读", b.summary().contains("recs=2 loaded=true"), "双档已载");
    s.add("同键分账", h1 == h2, "dev/DEV 同哈希");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12117_hash_flow() {
        let mut b = HashBook::new();
        assert_eq!(b.put(1, &[0x41], "a"), Err(UH_NO_TABLE));
        b.load_table();
        let h = b.put(1, &[0x41], "a").unwrap_or(0);
        assert_eq!(b.hash_of(1), Some(h));
        assert_eq!(b.put(1, &[0x42], "b"), Err(UH_DUP_ID));
    }

    #[test]
    fn unx12117_checks_green() {
        let s = run_unx12117_checks();
        assert!(s.all_passed(), "UNX-F12117 判据未全绿");
    }

    #[test]
    fn hash_stability() {
        let u = [0x61u32, 0x62, 0x63];
        assert_eq!(hash_units(&u), hash_units(&u));
        assert_eq!(hash_units(&u), hash_units(&[0x41, 0x42, 0x43]));
    }
}
