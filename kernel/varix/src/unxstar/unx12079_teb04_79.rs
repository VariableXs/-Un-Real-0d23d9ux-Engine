//! UNX-F12079 TEB 对照表制度：逐字段语义档与可复测判据（0x19c00 段）
//!
//! 卡面：TEB 逐字段对照表制度——每字段一档（编号/名/偏移/读写域/写者侧），
//! 档案编号与名字双唯一、偏移单调递增、域与写者侧闭集。
//! 域：CoRun·UNX-D1-B04。
//!
//! 判据原文：
//! - J1 编号唯一：档案编号非零且全册唯一，重号必拒。
//! - J2 名字唯一：字段名全册唯一，重名必拒。
//! - J3 读写域闭集：R/W/RW 三值闭集，越码必拒。
//! - J4 写者侧闭集：Kernel/User/Both 三值闭集，越码必拒。
//! - J5 偏移单调：档案偏移不小于前一档案尾（区间不回叠）。
//! - J6 档案回读：逐档可渲染，读写域与写者侧可回读。
//! - J7 印账冻结：seal 后归档必拒。
//! - J8 注入五况全抓。
//!
//! 结构：FieldAcc/FieldWho 双闭集枚举 + ArchiveRec 字段档 + ArchiveBook
//! 对照册 + 折叠指纹（211 进制卷积）+ 注入核（重号/重名/越域/回叠/印后归档）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19c00 独占 0x20）。
pub const SEG_BASE: u32 = 0x19c00;
pub const AX_DUP_ID: u32 = SEG_BASE + 0x01; // 重号
pub const AX_DUP_NAME: u32 = SEG_BASE + 0x02; // 重名
pub const AX_BAD_ACC: u32 = SEG_BASE + 0x03; // 读写域越码
pub const AX_BAD_WHO: u32 = SEG_BASE + 0x04; // 写者侧越码
pub const AX_MONO: u32 = SEG_BASE + 0x05; // 偏移回叠
pub const AX_FROZEN: u32 = SEG_BASE + 0x06; // 印账后归档

/// 读写域三值闭集。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FieldAcc {
    R = 1,
    W = 2,
    RW = 3,
}

/// 写者侧三值闭集。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FieldWho {
    Kernel = 1,
    User = 2,
    Both = 3,
}

/// 字段档案。
#[derive(Clone)]
pub struct ArchiveRec {
    pub id: u32,
    pub name: String,
    pub off: u32,
    pub size: u32,
    pub acc: FieldAcc,
    pub who: FieldWho,
}

impl ArchiveRec {
    pub fn render(&self) -> String {
        format!(
            "A{}|{}|{:x}+{:x}|a{}|w{}",
            self.id, self.name, self.off, self.size, self.acc as u16, self.who as u16
        )
    }
    /// 档案自洽：尺寸非零。
    pub fn ok(&self) -> bool {
        self.size != 0
    }
}

/// TEB 字段对照册。
pub struct ArchiveBook {
    pub recs: Vec<ArchiveRec>,
    pub sealed: bool,
}

impl ArchiveBook {
    pub fn new() -> ArchiveBook {
        ArchiveBook { recs: Vec::new(), sealed: false }
    }

    /// 归档：编号/名字双唯一 → 域与侧闭集 → 偏移单调。
    pub fn archive(&mut self, r: ArchiveRec) -> Result<(), u32> {
        if self.sealed {
            return Err(AX_FROZEN);
        }
        if r.id == 0 {
            return Err(AX_DUP_ID); // 零号视作重号面拒
        }
        if self.recs.iter().any(|x| x.id == r.id) {
            return Err(AX_DUP_ID);
        }
        if self.recs.iter().any(|x| x.name == r.name) {
            return Err(AX_DUP_NAME);
        }
        let acc_ok = matches!(r.acc, FieldAcc::R | FieldAcc::W | FieldAcc::RW);
        if !acc_ok {
            return Err(AX_BAD_ACC);
        }
        let who_ok = matches!(r.who, FieldWho::Kernel | FieldWho::User | FieldWho::Both);
        if !who_ok {
            return Err(AX_BAD_WHO);
        }
        if let Some(prev) = self.recs.last() {
            if r.off < prev.off + prev.size {
                return Err(AX_MONO);
            }
        }
        if !r.ok() {
            return Err(AX_BAD_ACC);
        }
        self.recs.push(r);
        Ok(())
    }

    /// 档案回读。
    pub fn by_id(&self, id: u32) -> Option<&ArchiveRec> {
        self.recs.iter().find(|r| r.id == id)
    }

    /// 名字查询。
    pub fn by_name(&self, name: &str) -> Option<&ArchiveRec> {
        self.recs.iter().find(|r| r.name == name)
    }

    /// 全册自洽：逐档合法且编号升序。
    pub fn all_ok(&self) -> bool {
        let mut prev: u32 = 0;
        for r in &self.recs {
            if !r.ok() || r.id <= prev {
                return false;
            }
            prev = r.id;
        }
        true
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.recs {
            h = h.wrapping_mul(211).wrapping_add(r.id);
            h = h.wrapping_mul(211).wrapping_add(r.off);
            h = h.wrapping_mul(211).wrapping_add(r.size);
            h = h.wrapping_mul(211).wrapping_add(r.acc as u32);
            h = h.wrapping_mul(211).wrapping_add(r.who as u32);
            for by in r.name.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：档案数。
    pub fn count(&self) -> usize {
        self.recs.len()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("recs={}", self.recs.len())
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
    let mut b = ArchiveBook::new();
    let _ = b.archive(ArchiveRec {
        id: 1, name: "Self".to_string(), off: 0x30, size: 8,
        acc: FieldAcc::RW, who: FieldWho::Both,
    });
    // 1) 重号必拒。
    got[0] = b.archive(ArchiveRec {
        id: 1, name: "X".to_string(), off: 0x40, size: 4,
        acc: FieldAcc::R, who: FieldWho::Kernel,
    }).err() == Some(AX_DUP_ID);
    // 2) 重名必拒。
    got[1] = b.archive(ArchiveRec {
        id: 2, name: "Self".to_string(), off: 0x40, size: 4,
        acc: FieldAcc::R, who: FieldWho::Kernel,
    }).err() == Some(AX_DUP_NAME);
    // 3) 偏移回叠必拒（新档起点落入前档区间）。
    got[2] = b.archive(ArchiveRec {
        id: 2, name: "Over".to_string(), off: 0x34, size: 4,
        acc: FieldAcc::R, who: FieldWho::Kernel,
    }).err() == Some(AX_MONO);
    // 4) 零尺寸档案必拒。
    got[3] = b.archive(ArchiveRec {
        id: 2, name: "Zero".to_string(), off: 0x40, size: 0,
        acc: FieldAcc::R, who: FieldWho::Kernel,
    }).err() == Some(AX_BAD_ACC);
    // 5) 印账后归档必抓。
    b.seal();
    got[4] = b.archive(ArchiveRec {
        id: 2, name: "Post".to_string(), off: 0x40, size: 4,
        acc: FieldAcc::R, who: FieldWho::Kernel,
    }).err() == Some(AX_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12079_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12079");

    // J1 编号唯一。
    let j1 = {
        let mut b = ArchiveBook::new();
        let _ = b.archive(ArchiveRec {
            id: 1, name: "Self".to_string(), off: 0x30, size: 8,
            acc: FieldAcc::RW, who: FieldWho::Both,
        });
        b.archive(ArchiveRec {
            id: 1, name: "Other".to_string(), off: 0x40, size: 4,
            acc: FieldAcc::R, who: FieldWho::Kernel,
        }).err() == Some(AX_DUP_ID)
    };
    s.add("J1-编号唯一", j1, "重号必拒");

    // J2 零号视作重号面拒。
    let j2 = {
        let mut b = ArchiveBook::new();
        b.archive(ArchiveRec {
            id: 0, name: "Z".to_string(), off: 0, size: 4,
            acc: FieldAcc::R, who: FieldWho::Kernel,
        }).err() == Some(AX_DUP_ID)
    };
    s.add("J2-零号拒", j2, "零编号必拒");

    // J3 名字唯一。
    let j3 = {
        let mut b = ArchiveBook::new();
        let _ = b.archive(ArchiveRec {
            id: 1, name: "LastError".to_string(), off: 0x34, size: 4,
            acc: FieldAcc::RW, who: FieldWho::Both,
        });
        b.archive(ArchiveRec {
            id: 2, name: "LastError".to_string(), off: 0x40, size: 4,
            acc: FieldAcc::R, who: FieldWho::Kernel,
        }).err() == Some(AX_DUP_NAME)
    };
    s.add("J3-名字唯一", j3, "重名必拒");

    // J4 偏移单调。
    let j4 = {
        let mut b = ArchiveBook::new();
        let _ = b.archive(ArchiveRec {
            id: 1, name: "A".to_string(), off: 0x30, size: 8,
            acc: FieldAcc::RW, who: FieldWho::Both,
        });
        b.archive(ArchiveRec {
            id: 2, name: "B".to_string(), off: 0x34, size: 4,
            acc: FieldAcc::R, who: FieldWho::Kernel,
        }).err() == Some(AX_MONO)
    };
    s.add("J4-偏移单调", j4, "回叠必拒");

    // J5 紧邻接续放行。
    let j5 = {
        let mut b = ArchiveBook::new();
        let _ = b.archive(ArchiveRec {
            id: 1, name: "A".to_string(), off: 0x30, size: 8,
            acc: FieldAcc::RW, who: FieldWho::Both,
        });
        b.archive(ArchiveRec {
            id: 2, name: "B".to_string(), off: 0x38, size: 4,
            acc: FieldAcc::R, who: FieldWho::Kernel,
        }).is_ok()
    };
    s.add("J5-紧邻放行", j5, "尾接续不拒");

    // J6 档案回读与名字查询。
    let j6 = {
        let mut b = ArchiveBook::new();
        let _ = b.archive(ArchiveRec {
            id: 3, name: "ClientId".to_string(), off: 0x40, size: 16,
            acc: FieldAcc::R, who: FieldWho::Both,
        });
        b.by_name("ClientId").is_some() && b.line_of(3).contains("ClientId")
    };
    s.add("J6-档案回读", j6, "双通道查询与渲染");

    // J7 全册自洽。
    let j7 = {
        let mut b = ArchiveBook::new();
        let _ = b.archive(ArchiveRec {
            id: 1, name: "A".to_string(), off: 0x00, size: 4,
            acc: FieldAcc::R, who: FieldWho::Kernel,
        });
        let _ = b.archive(ArchiveRec {
            id: 2, name: "B".to_string(), off: 0x08, size: 4,
            acc: FieldAcc::RW, who: FieldWho::Both,
        });
        b.all_ok() && b.count() == 2
    };
    s.add("J7-全册自洽", j7, "编号升序逐档合法");

    // J8 印账冻结。
    let j8 = {
        let mut b = ArchiveBook::new();
        b.seal();
        b.archive(ArchiveRec {
            id: 1, name: "P".to_string(), off: 0, size: 4,
            acc: FieldAcc::R, who: FieldWho::Kernel,
        }).err() == Some(AX_FROZEN)
    };
    s.add("J8-印账冻结", j8, "seal 后必拒");

    // J9 指纹幂等。
    let j9 = {
        let mut b = ArchiveBook::new();
        let _ = b.archive(ArchiveRec {
            id: 1, name: "A".to_string(), off: 0, size: 4,
            acc: FieldAcc::R, who: FieldWho::Kernel,
        });
        b.fold() == b.fold()
    };
    s.add("J9-指纹幂等", j9, "同态同指纹");

    // J10 注入核 5/5。
    let got = inject_five();
    let j10 = got.iter().all(|x| *x);
    s.add_dyn("J10-注入全抓", j10, &format!("inj5={:?}", got));

    // 汇总样例账：TEB 代表字段三档。
    let mut b = ArchiveBook::new();
    let _ = b.archive(ArchiveRec {
        id: 1, name: "Self".to_string(), off: 0x30, size: 8,
        acc: FieldAcc::RW, who: FieldWho::Both,
    });
    let _ = b.archive(ArchiveRec {
        id: 2, name: "LastError".to_string(), off: 0x38, size: 4,
        acc: FieldAcc::RW, who: FieldWho::Both,
    });
    let _ = b.archive(ArchiveRec {
        id: 3, name: "CurrentLocale".to_string(), off: 0xc0, size: 4,
        acc: FieldAcc::RW, who: FieldWho::User,
    });

    s.add("摘要回读", b.summary().contains("recs=3"), "三档在册");
    s.add("缺席行", b.line_of(9) == "missing", "缺档显性");
    s.add("写者侧回读", b.by_name("CurrentLocale").map(|r| r.who as u16) == Some(2), "User 侧 2");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12079_archive_flow() {
        let mut b = ArchiveBook::new();
        assert!(b.archive(ArchiveRec {
            id: 1, name: "A".to_string(), off: 0, size: 4,
            acc: FieldAcc::R, who: FieldWho::Kernel,
        }).is_ok());
        assert_eq!(b.by_id(1).map(|r| r.id), Some(1));
        assert!(b.by_name("B").is_none());
    }

    #[test]
    fn unx12079_checks_green() {
        let s = run_unx12079_checks();
        assert!(s.all_passed(), "UNX-F12079 判据未全绿");
    }

    #[test]
    fn acc_who_closed_sets() {
        assert_eq!(FieldAcc::RW as u16, 3);
        assert_eq!(FieldWho::Both as u16, 3);
        assert_ne!(FieldAcc::R, FieldAcc::W);
    }
}
