//! UNX-F12097 “D1 管结构协议、D3 管名字存储”分界判据与联签文本（0x19e40 段）
//!
//! 卡面：D1/D3 分界契约账——契约条款逐条归属 D1（结构协议）或 D3
//! （名字存储），错主必拒；联签记录每条款双域各签一次。
//! 域：CoRun·UNX-D1-B05。
//!
//! 判据原文：
//! - J1 条款闭集：条款编号非零唯一，重号必拒。
//! - J2 归属闭集：归属域仅 D1/D3 两值。
//! - J3 错主拒：结构协议条款误归 D3（或名字存储条款误归 D1）必拒。
//! - J4 联签单次：同条款同域二次联签必拒。
//! - J5 联签双齐：每条款 D1/D3 双签齐备才算闭合。
//! - J6 联签文本：文本含分界口径关键词。
//! - J7 印账冻结：seal 后一切操作必拒。
//! - J8 注入五况全抓。
//!
//! 结构：Domain 两域枚举 + Clause 条款 + CosignBook 契约账 +
//! 折叠指纹（211 进制卷积）+ 注入核（零号/重号/错主/双签/印后签）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19e40 独占 0x20）。
pub const SEG_BASE: u32 = 0x19e40;
pub const JB_ZERO: u32 = SEG_BASE + 0x01; // 零号条款
pub const JB_DUP: u32 = SEG_BASE + 0x02; // 重号
pub const JB_WRONG_OWNER: u32 = SEG_BASE + 0x03; // 错主
pub const JB_DOUBLE_SIGN: u32 = SEG_BASE + 0x04; // 双签
pub const JB_FROZEN: u32 = SEG_BASE + 0x05; // 印账后签

/// 两域枚举。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Domain {
    D1 = 1,
    D3 = 2,
}

/// 条款类别（决定正确归属）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClauseKind {
    StructProtocol = 1, // 应归 D1
    NameStorage = 2,    // 应归 D3
}

impl ClauseKind {
    /// 正确归属域。
    pub fn right_owner(&self) -> Domain {
        match self {
            ClauseKind::StructProtocol => Domain::D1,
            ClauseKind::NameStorage => Domain::D3,
        }
    }
}

/// 条款。
#[derive(Clone)]
pub struct Clause {
    pub id: u32,
    pub kind: ClauseKind,
    pub owner: Domain,
    pub text: String,
}

impl Clause {
    /// 归属正确性。
    pub fn owner_ok(&self) -> bool {
        self.owner == self.kind.right_owner()
    }
    pub fn render(&self) -> String {
        format!("C{}|K{}|D{}", self.id, self.kind as u16, self.owner as u16)
    }
}

/// 契约账。
pub struct CosignBook {
    pub clauses: Vec<Clause>,
    pub signs: Vec<(u32, Domain)>, // (条款号, 签域)
    pub sealed: bool,
}

impl CosignBook {
    pub fn new() -> CosignBook {
        CosignBook { clauses: Vec::new(), signs: Vec::new(), sealed: false }
    }

    /// 立条款：零号重号必拒，错主必拒。
    pub fn add(&mut self, c: Clause) -> Result<(), u32> {
        if self.sealed {
            return Err(JB_FROZEN);
        }
        if c.id == 0 {
            return Err(JB_ZERO);
        }
        if self.clauses.iter().any(|x| x.id == c.id) {
            return Err(JB_DUP);
        }
        if !c.owner_ok() {
            return Err(JB_WRONG_OWNER);
        }
        self.clauses.push(c);
        Ok(())
    }

    /// 联签：同条款同域二次必拒。
    pub fn sign(&mut self, clause_id: u32, by: Domain) -> Result<(), u32> {
        if self.sealed {
            return Err(JB_FROZEN);
        }
        if !self.clauses.iter().any(|c| c.id == clause_id) {
            return Err(JB_ZERO); // 未知条款视作零面
        }
        if self.signs.iter().any(|s| s.0 == clause_id && s.1 == by) {
            return Err(JB_DOUBLE_SIGN);
        }
        self.signs.push((clause_id, by));
        Ok(())
    }

    /// 条款双签齐备判定。
    pub fn fully_signed(&self, clause_id: u32) -> bool {
        self.signs.iter().any(|s| s.0 == clause_id && s.1 == Domain::D1)
            && self.signs.iter().any(|s| s.0 == clause_id && s.1 == Domain::D3)
    }

    /// 全账闭合。
    pub fn all_closed(&self) -> bool {
        !self.clauses.is_empty()
            && self.clauses.iter().all(|c| self.fully_signed(c.id))
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for c in &self.clauses {
            h = h.wrapping_mul(211).wrapping_add(c.id);
            h = h.wrapping_mul(211).wrapping_add(c.kind as u32);
            h = h.wrapping_mul(211).wrapping_add(c.owner as u32);
            for by in c.text.bytes() {
                h = h.wrapping_mul(211).wrapping_add(by as u32);
            }
        }
        for s in &self.signs {
            h = h.wrapping_mul(211).wrapping_add(s.0);
            h = h.wrapping_mul(211).wrapping_add(s.1 as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("clauses={} signs={}", self.clauses.len(), self.signs.len())
    }

    /// 诊断面：联签文本。
    pub fn cosign_text(&self) -> String {
        "D1=struct-protocol;D3=name-storage".to_string()
    }

    /// 诊断面：条款渲染。
    pub fn line_of(&self, id: u32) -> String {
        match self.clauses.iter().find(|c| c.id == id) {
            Some(c) => c.render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = CosignBook::new();
    // 1) 零号条款必拒。
    got[0] = b.add(Clause {
        id: 0, kind: ClauseKind::StructProtocol, owner: Domain::D1,
        text: "x".to_string(),
    }).err() == Some(JB_ZERO);
    // 2) 重号必拒。
    let _ = b.add(Clause {
        id: 1, kind: ClauseKind::StructProtocol, owner: Domain::D1,
        text: "layout".to_string(),
    });
    got[1] = b.add(Clause {
        id: 1, kind: ClauseKind::NameStorage, owner: Domain::D3,
        text: "y".to_string(),
    }).err() == Some(JB_DUP);
    // 3) 错主必拒（名字存储条款误归 D1）。
    got[2] = b.add(Clause {
        id: 2, kind: ClauseKind::NameStorage, owner: Domain::D1,
        text: "bad".to_string(),
    }).err() == Some(JB_WRONG_OWNER);
    // 4) 双签必拒。
    let _ = b.sign(1, Domain::D1);
    got[3] = b.sign(1, Domain::D1).err() == Some(JB_DOUBLE_SIGN);
    // 5) 印账后签必抓。
    b.seal();
    got[4] = b.sign(1, Domain::D3).err() == Some(JB_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12097_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12097");

    // J1 零号条款拒。
    let j1 = {
        let mut b = CosignBook::new();
        b.add(Clause {
            id: 0, kind: ClauseKind::StructProtocol, owner: Domain::D1,
            text: "z".to_string(),
        }).err() == Some(JB_ZERO)
    };
    s.add("J1-零号拒", j1, "零号必拒");

    // J2 重号拒。
    let j2 = {
        let mut b = CosignBook::new();
        let _ = b.add(Clause {
            id: 1, kind: ClauseKind::StructProtocol, owner: Domain::D1,
            text: "a".to_string(),
        });
        b.add(Clause {
            id: 1, kind: ClauseKind::StructProtocol, owner: Domain::D1,
            text: "b".to_string(),
        }).err() == Some(JB_DUP)
    };
    s.add("J2-重号拒", j2, "同号二次必拒");

    // J3 错主拒：双向（D1 条款误归 D3 亦拒）。
    let j3 = {
        let mut b = CosignBook::new();
        b.add(Clause {
            id: 1, kind: ClauseKind::StructProtocol, owner: Domain::D3,
            text: "bad1".to_string(),
        }).err() == Some(JB_WRONG_OWNER)
            && b.add(Clause {
                id: 2, kind: ClauseKind::NameStorage, owner: Domain::D1,
                text: "bad2".to_string(),
            }).err() == Some(JB_WRONG_OWNER)
    };
    s.add("J3-错主双向拒", j3, "双向错主皆拒");

    // J4 正主放行。
    let j4 = {
        let mut b = CosignBook::new();
        b.add(Clause {
            id: 1, kind: ClauseKind::StructProtocol, owner: Domain::D1,
            text: "layout".to_string(),
        }).is_ok()
            && b.add(Clause {
                id: 2, kind: ClauseKind::NameStorage, owner: Domain::D3,
                text: "store".to_string(),
            }).is_ok()
    };
    s.add("J4-正主放行", j4, "双条款各归其主");

    // J5 联签单次与双齐。
    let j5 = {
        let mut b = CosignBook::new();
        let _ = b.add(Clause {
            id: 3, kind: ClauseKind::StructProtocol, owner: Domain::D1,
            text: "c".to_string(),
        });
        let _ = b.sign(3, Domain::D1);
        b.sign(3, Domain::D1).err() == Some(JB_DOUBLE_SIGN)
            && !b.fully_signed(3)
            && b.sign(3, Domain::D3).is_ok()
            && b.fully_signed(3)
    };
    s.add("J5-联签双齐", j5, "双签闭合可判");

    // J6 未知条款签拒。
    let j6 = {
        let mut b = CosignBook::new();
        b.sign(9, Domain::D1).err() == Some(JB_ZERO)
    };
    s.add("J6-未知条款拒", j6, "签未知条款必拒");

    // J7 全账闭合聚合。
    let j7 = {
        let mut b = CosignBook::new();
        let _ = b.add(Clause {
            id: 1, kind: ClauseKind::StructProtocol, owner: Domain::D1,
            text: "a".to_string(),
        });
        let _ = b.add(Clause {
            id: 2, kind: ClauseKind::NameStorage, owner: Domain::D3,
            text: "b".to_string(),
        });
        let _ = b.sign(1, Domain::D1);
        let _ = b.sign(1, Domain::D3);
        let _ = b.sign(2, Domain::D1);
        let _ = b.sign(2, Domain::D3);
        b.all_closed()
    };
    s.add("J7-全账闭合", j7, "双条款四签全齐");

    // J8 联签文本关键词。
    let j8 = {
        let b = CosignBook::new();
        let t = b.cosign_text();
        t.contains("D1=struct-protocol") && t.contains("D3=name-storage")
    };
    s.add("J8-联签文本", j8, "分界口径在文");

    // J9 印账冻结。
    let j9 = {
        let mut b = CosignBook::new();
        b.seal();
        b.add(Clause {
            id: 1, kind: ClauseKind::StructProtocol, owner: Domain::D1,
            text: "p".to_string(),
        }).err() == Some(JB_FROZEN)
            && b.sign(1, Domain::D1).err() == Some(JB_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后皆拒");

    // J10 指纹幂等。
    let j10 = {
        let mut b = CosignBook::new();
        let _ = b.add(Clause {
            id: 1, kind: ClauseKind::StructProtocol, owner: Domain::D1,
            text: "a".to_string(),
        });
        b.fold() == b.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = CosignBook::new();
    let _ = b.add(Clause {
        id: 1, kind: ClauseKind::StructProtocol, owner: Domain::D1,
        text: "OBJECT_ATTRIBUTES 结构协议".to_string(),
    });
    let _ = b.add(Clause {
        id: 2, kind: ClauseKind::NameStorage, owner: Domain::D3,
        text: "对象名存储".to_string(),
    });
    let _ = b.sign(1, Domain::D1);
    let _ = b.sign(2, Domain::D3);

    s.add("摘要回读", b.summary().contains("signs=2"), "双签在账");
    s.add("条款渲染", b.line_of(2).contains("D2"), "D3 渲染码 2");
    s.add("缺席行", b.line_of(9) == "missing", "缺条款显性");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12097_clause_flow() {
        let mut b = CosignBook::new();
        assert!(b.add(Clause {
            id: 1, kind: ClauseKind::StructProtocol, owner: Domain::D1,
            text: "a".to_string(),
        }).is_ok());
        assert!(b.sign(1, Domain::D1).is_ok());
        assert_eq!(b.sign(1, Domain::D1), Err(JB_DOUBLE_SIGN));
    }

    #[test]
    fn unx12097_checks_green() {
        let s = run_unx12097_checks();
        assert!(s.all_passed(), "UNX-F12097 判据未全绿");
    }

    #[test]
    fn owner_mapping() {
        assert_eq!(ClauseKind::StructProtocol.right_owner(), Domain::D1);
        assert_eq!(ClauseKind::NameStorage.right_owner(), Domain::D3);
    }
}
