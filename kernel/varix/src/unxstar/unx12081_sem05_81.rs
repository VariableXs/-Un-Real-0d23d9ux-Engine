//! UNX-F12081 OBJECT_ATTRIBUTES 结构语义实装（0x19c40 段）
//!
//! 卡面：OBJECT_ATTRIBUTES x64 结构（48 字节六字段）语义实装——长度锚、
//! 保留位、名/根互备、UTF-16 名长偶数约束、登记册唯一性。
//! 域：CoRun·UNX-D1-B05。
//!
//! 判据原文：
//! - J1 长度锚：x64 Length 必须恰为 48，违者必拒。
//! - J2 保留位零：Attributes 高段保留位（≥0x00010000）非零必拒。
//! - J3 名/根互备：ObjectName 与 RootDirectory 双空必拒（相对打开底座
//!   至少持其一）。
//! - J4 名长偶数：UTF-16 名缓冲字节数必为偶数。
//! - J5 登记册唯一：实例编号非零唯一，重号必拒。
//! - J6 正路径：合法实例逐字段回读。
//! - J7 印账冻结：seal 后登记必拒。
//! - J8 注入五况全抓。
//!
//! 结构：ObjAttr 结构档 + ObjAttrBook 登记册 + 折叠指纹（211 进制卷积）+
//! 注入核（坏长度/保留位/双空/奇名长/印后登记）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19c40 独占 0x20）。
pub const SEG_BASE: u32 = 0x19c40;
pub const OA_BAD_LEN: u32 = SEG_BASE + 0x01; // 长度锚失配
pub const OA_RESERVED: u32 = SEG_BASE + 0x02; // 保留位非零
pub const OA_NULL_BOTH: u32 = SEG_BASE + 0x03; // 名根双空
pub const OA_ODD_NAME: u32 = SEG_BASE + 0x04; // 奇名长
pub const OA_DUP_ID: u32 = SEG_BASE + 0x05; // 重号（含零号）
pub const OA_FROZEN: u32 = SEG_BASE + 0x06; // 印账后登记

/// x64 结构长度锚。
pub const X64_LEN: u32 = 48;

/// OBJECT_ATTRIBUTES 结构档。
#[derive(Clone)]
pub struct ObjAttr {
    pub id: u32,
    pub length: u32,
    pub root: u32,
    pub name_present: bool,
    pub name_len: u32,
    pub attrs: u32,
    pub has_sd: bool,
    pub has_sqos: bool,
}

impl ObjAttr {
    /// 结构静态校验：长度 → 保留位 → 名根互备 → 名长偶数。
    pub fn validate(&self) -> Result<(), u32> {
        if self.length != X64_LEN {
            return Err(OA_BAD_LEN);
        }
        if self.attrs >= 0x0001_0000 {
            return Err(OA_RESERVED);
        }
        if !self.name_present && self.root == 0 {
            return Err(OA_NULL_BOTH);
        }
        if self.name_present && self.name_len % 2 != 0 {
            return Err(OA_ODD_NAME);
        }
        Ok(())
    }
    pub fn render(&self) -> String {
        format!(
            "O{}|len{}|root{}|n{}|at{:x}",
            self.id, self.length, self.root, self.name_present as u8, self.attrs
        )
    }
}

/// 实例登记册。
pub struct ObjAttrBook {
    pub recs: Vec<ObjAttr>,
    pub sealed: bool,
}

impl ObjAttrBook {
    pub fn new() -> ObjAttrBook {
        ObjAttrBook { recs: Vec::new(), sealed: false }
    }

    /// 登记：编号唯一 → 结构校验。
    pub fn put(&mut self, a: ObjAttr) -> Result<(), u32> {
        if self.sealed {
            return Err(OA_FROZEN);
        }
        if a.id == 0 || self.recs.iter().any(|r| r.id == a.id) {
            return Err(OA_DUP_ID);
        }
        a.validate()?;
        self.recs.push(a);
        Ok(())
    }

    /// 回读。
    pub fn by_id(&self, id: u32) -> Option<&ObjAttr> {
        self.recs.iter().find(|r| r.id == id)
    }

    /// 全册自洽。
    pub fn all_ok(&self) -> bool {
        self.recs.iter().all(|r| r.validate().is_ok())
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.id);
            h = h.wrapping_mul(211).wrapping_add(r.length);
            h = h.wrapping_mul(211).wrapping_add(r.root);
            h = h.wrapping_mul(211).wrapping_add(r.attrs);
            h = h.wrapping_mul(211).wrapping_add(r.name_len);
            h = h.wrapping_mul(211)
                .wrapping_add((r.name_present as u32) << 1 | r.has_sd as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("recs={} sd={}", self.recs.len(), self.recs.iter().filter(|r| r.has_sd).count())
    }

    /// 诊断面：渲染行。
    pub fn line_of(&self, id: u32) -> String {
        match self.by_id(id) {
            Some(r) => r.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = ObjAttrBook::new();
    // 1) 坏长度（x86 的 24）必拒。
    got[0] = b.put(ObjAttr {
        id: 1, length: 24, root: 0, name_present: true, name_len: 8,
        attrs: 0, has_sd: false, has_sqos: false,
    }).err() == Some(OA_BAD_LEN);
    // 2) 保留位非零必拒。
    got[1] = b.put(ObjAttr {
        id: 1, length: X64_LEN, root: 0, name_present: true, name_len: 8,
        attrs: 0x0001_0000, has_sd: false, has_sqos: false,
    }).err() == Some(OA_RESERVED);
    // 3) 名根双空必拒。
    got[2] = b.put(ObjAttr {
        id: 1, length: X64_LEN, root: 0, name_present: false, name_len: 0,
        attrs: 0, has_sd: false, has_sqos: false,
    }).err() == Some(OA_NULL_BOTH);
    // 4) 奇名长必拒。
    got[3] = b.put(ObjAttr {
        id: 1, length: X64_LEN, root: 0, name_present: true, name_len: 7,
        attrs: 0, has_sd: false, has_sqos: false,
    }).err() == Some(OA_ODD_NAME);
    // 5) 重号 + 印账后登记必抓。
    let good = ObjAttr {
        id: 1, length: X64_LEN, root: 0, name_present: true, name_len: 8,
        attrs: 0, has_sd: false, has_sqos: false,
    };
    let _ = b.put(good.clone());
    let dup_hit = b.put(good).err() == Some(OA_DUP_ID);
    b.seal();
    let frozen_hit = b.put(ObjAttr {
        id: 2, length: X64_LEN, root: 0, name_present: true, name_len: 8,
        attrs: 0, has_sd: false, has_sqos: false,
    }).err() == Some(OA_FROZEN);
    got[4] = dup_hit && frozen_hit;
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12081_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12081");

    // J1 长度锚。
    let j1 = {
        let mut b = ObjAttrBook::new();
        b.put(ObjAttr {
            id: 1, length: 24, root: 0, name_present: true, name_len: 8,
            attrs: 0, has_sd: false, has_sqos: false,
        }).err() == Some(OA_BAD_LEN)
    };
    s.add("J1-长度锚", j1, "非 48 必拒");

    // J2 保留位零。
    let j2 = {
        let mut b = ObjAttrBook::new();
        b.put(ObjAttr {
            id: 1, length: X64_LEN, root: 0, name_present: true, name_len: 8,
            attrs: 0x0002_0000, has_sd: false, has_sqos: false,
        }).err() == Some(OA_RESERVED)
    };
    s.add("J2-保留位零", j2, "高段非零必拒");

    // J3 名根互备。
    let j3 = {
        let mut b = ObjAttrBook::new();
        b.put(ObjAttr {
            id: 1, length: X64_LEN, root: 0, name_present: false, name_len: 0,
            attrs: 0, has_sd: false, has_sqos: false,
        }).err() == Some(OA_NULL_BOTH)
    };
    s.add("J3-名根互备", j3, "双空必拒");

    // J4 名长偶数。
    let j4 = {
        let a1 = ObjAttr {
            id: 1, length: X64_LEN, root: 0, name_present: true, name_len: 7,
            attrs: 0, has_sd: false, has_sqos: false,
        };
        let a2 = ObjAttr {
            id: 1, length: X64_LEN, root: 0, name_present: true, name_len: 6,
            attrs: 0, has_sd: false, has_sqos: false,
        };
        a1.validate().err() == Some(OA_ODD_NAME) && a2.validate().is_ok()
    };
    s.add("J4-名长偶数", j4, "奇拒偶过");

    // J5 编号唯一（含零号）。
    let j5 = {
        let mut b = ObjAttrBook::new();
        b.put(ObjAttr {
            id: 0, length: X64_LEN, root: 0, name_present: true, name_len: 8,
            attrs: 0, has_sd: false, has_sqos: false,
        }).err() == Some(OA_DUP_ID)
    };
    s.add("J5-零号拒", j5, "零编号必拒");

    // J6 正路径回读。
    let j6 = {
        let mut b = ObjAttrBook::new();
        let _ = b.put(ObjAttr {
            id: 3, length: X64_LEN, root: 7, name_present: true, name_len: 8,
            attrs: 0x40, has_sd: true, has_sqos: true,
        });
        b.by_id(3).is_some()
            && b.line_of(3).contains("root7")
            && b.summary().contains("sd=1")
    };
    s.add("J6-正路径回读", j6, "逐字段可回读");

    // J7 重号拒。
    let j7 = {
        let mut b = ObjAttrBook::new();
        let a = ObjAttr {
            id: 9, length: X64_LEN, root: 0, name_present: true, name_len: 8,
            attrs: 0, has_sd: false, has_sqos: false,
        };
        let _ = b.put(a.clone());
        b.put(a).err() == Some(OA_DUP_ID)
    };
    s.add("J7-重号拒", j7, "二次登记必拒");

    // J8 印账冻结。
    let j8 = {
        let mut b = ObjAttrBook::new();
        b.seal();
        b.put(ObjAttr {
            id: 1, length: X64_LEN, root: 0, name_present: true, name_len: 8,
            attrs: 0, has_sd: false, has_sqos: false,
        }).err() == Some(OA_FROZEN)
    };
    s.add("J8-印账冻结", j8, "seal 后必拒");

    // J9 指纹幂等。
    let j9 = {
        let mut b = ObjAttrBook::new();
        let _ = b.put(ObjAttr {
            id: 1, length: X64_LEN, root: 0, name_present: true, name_len: 8,
            attrs: 0, has_sd: false, has_sqos: false,
        });
        b.fold() == b.fold()
    };
    s.add("J9-指纹幂等", j9, "同态同指纹");

    // J10 注入核 5/5。
    let got = inject_five();
    let j10 = got.iter().all(|x| *x);
    s.add_dyn("J10-注入全抓", j10, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = ObjAttrBook::new();
    let _ = b.put(ObjAttr {
        id: 1, length: X64_LEN, root: 0, name_present: true, name_len: 8,
        attrs: 0x40, has_sd: false, has_sqos: false,
    });
    let _ = b.put(ObjAttr {
        id: 2, length: X64_LEN, root: 5, name_present: true, name_len: 12,
        attrs: 0, has_sd: true, has_sqos: false,
    });

    s.add("全册自洽", b.all_ok(), "逐档过校验");
    s.add("摘要回读", b.summary().contains("recs=2"), "双档在册");
    s.add("缺席行", b.line_of(9) == "missing", "缺档显性");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12081_validate_flow() {
        let good = ObjAttr {
            id: 1, length: 48, root: 0, name_present: true, name_len: 8,
            attrs: 0x40, has_sd: false, has_sqos: false,
        };
        assert!(good.validate().is_ok());
        let bad = ObjAttr { length: 24, ..good };
        assert_eq!(bad.validate(), Err(OA_BAD_LEN));
    }

    #[test]
    fn unx12081_checks_green() {
        let s = run_unx12081_checks();
        assert!(s.all_passed(), "UNX-F12081 判据未全绿");
    }

    #[test]
    fn book_dup_and_render() {
        let mut b = ObjAttrBook::new();
        let a = ObjAttr {
            id: 4, length: 48, root: 0, name_present: true, name_len: 8,
            attrs: 0, has_sd: false, has_sqos: false,
        };
        assert!(b.put(a.clone()).is_ok());
        assert_eq!(b.put(a), Err(OA_DUP_ID));
        assert!(b.line_of(4).contains("O4"));
    }
}
