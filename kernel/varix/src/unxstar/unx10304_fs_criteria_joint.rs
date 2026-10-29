//! UNX-F10304 fs 能力面判据联签（VFS/路径/文件族判据互认）（0x1b080 段）
//!
//! 锚点：UNX-F10304-J1 判据互认——VFS/路径/文件三族判据逐条双签且
//! 结论一致，族内缺额或结论冲突即互认失败。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：联签判据册（族号分桶，甲/乙双侧裁决位独立可翻，双签后不可
//! 重签）+ 互认核（双侧同裁才互认）+ 族覆盖核（每族达锚且全互认）+
//! 注入核（结论冲突/非法族号/未登记即签/缺额四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b080 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b080;
pub const FC_DUP: u32 = SEG_BASE + 0x01; // 判据号重号
pub const FC_EMPTY: u32 = SEG_BASE + 0x02; // 判据题空
pub const FC_BADFAM: u32 = SEG_BASE + 0x03; // 族号非法
pub const FC_NOCRIT: u32 = SEG_BASE + 0x04; // 未登记即签
pub const FC_SIGNED: u32 = SEG_BASE + 0x05; // 已签再签
pub const FC_CAP: u32 = SEG_BASE + 0x06; // 册超容
pub const FC_UNSIGNED: u32 = SEG_BASE + 0x07; // 未双签不可裁互认

/// 族号：1=VFS 2=路径 3=文件。
pub const FAM_VFS: u8 = 1;
pub const FAM_PATH: u8 = 2;
pub const FAM_FILE: u8 = 3;

/// 每族判据锚数。
pub const FAM_NEED: usize = 2;
/// 判据册定容。
pub const CAP: usize = 12;

/// 单条联签判据。
#[derive(Clone)]
pub struct JointCriterion {
    pub cid: u32,
    pub family: u8,
    pub title: String,
    pub a_verdict: Option<bool>,
    pub b_verdict: Option<bool>,
}

impl JointCriterion {
    /// 双侧已签。
    pub fn both_signed(&self) -> bool {
        self.a_verdict.is_some() && self.b_verdict.is_some()
    }

    /// 结论冲突：双签但裁决不等。
    pub fn conflict(&self) -> bool {
        match (self.a_verdict, self.b_verdict) {
            (Some(a), Some(b)) => a != b,
            _ => false,
        }
    }
}

/// fs 能力面判据联签册。
pub struct CapabilityJoint {
    pub crits: Vec<JointCriterion>,
}

impl CapabilityJoint {
    pub fn new() -> CapabilityJoint {
        CapabilityJoint { crits: Vec::new() }
    }

    fn find(&self, cid: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.crits.len() {
            if self.crits[k].cid == cid {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 登记判据：非法族、重号、空题、超容均拒。
    pub fn register(&mut self, cid: u32, family: u8, title: &str) -> Result<(), u32> {
        if family != FAM_VFS && family != FAM_PATH && family != FAM_FILE {
            return Err(FC_BADFAM);
        }
        if title.is_empty() {
            return Err(FC_EMPTY);
        }
        if cid == 0 || self.find(cid).is_some() {
            return Err(FC_DUP);
        }
        if self.crits.len() >= CAP {
            return Err(FC_CAP);
        }
        self.crits.push(JointCriterion {
            cid,
            family,
            title: title.to_string(),
            a_verdict: None,
            b_verdict: None,
        });
        Ok(())
    }

    /// 甲侧裁决：未登记或已签均拒。
    pub fn a_sign(&mut self, cid: u32, ok: bool) -> Result<(), u32> {
        match self.find(cid) {
            None => Err(FC_NOCRIT),
            Some(k) => {
                if self.crits[k].a_verdict.is_some() {
                    return Err(FC_SIGNED);
                }
                self.crits[k].a_verdict = Some(ok);
                Ok(())
            }
        }
    }

    /// 乙侧裁决：未登记或已签均拒。
    pub fn b_sign(&mut self, cid: u32, ok: bool) -> Result<(), u32> {
        match self.find(cid) {
            None => Err(FC_NOCRIT),
            Some(k) => {
                if self.crits[k].b_verdict.is_some() {
                    return Err(FC_SIGNED);
                }
                self.crits[k].b_verdict = Some(ok);
                Ok(())
            }
        }
    }

    /// 互认裁断：双签且同裁才认。
    pub fn mutual(&self, cid: u32) -> Result<bool, u32> {
        match self.find(cid) {
            None => Err(FC_NOCRIT),
            Some(k) => {
                let c = &self.crits[k];
                if !c.both_signed() {
                    return Err(FC_UNSIGNED);
                }
                Ok(!c.conflict())
            }
        }
    }

    /// 结论冲突清单。
    pub fn conflicts(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.crits.len() {
            if self.crits[k].conflict() {
                v.push(self.crits[k].cid);
            }
            k += 1;
        }
        v
    }

    /// 族覆盖：族内达锚且全数互认。
    pub fn family_covered(&self, family: u8) -> bool {
        let in_fam: Vec<&JointCriterion> = self.crits.iter().filter(|c| c.family == family).collect();
        in_fam.len() >= FAM_NEED && in_fam.iter().all(|c| c.both_signed() && !c.conflict())
    }

    /// 三族全覆盖。
    pub fn all_covered(&self) -> bool {
        self.family_covered(FAM_VFS)
            && self.family_covered(FAM_PATH)
            && self.family_covered(FAM_FILE)
    }

    /// 册折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b0800;
        h = h.wrapping_mul(31).wrapping_add(self.crits.len() as u32);
        let mut k = 0usize;
        while k < self.crits.len() {
            let c = &self.crits[k];
            h = h.wrapping_mul(31).wrapping_add(c.cid);
            h = h.wrapping_mul(31).wrapping_add(c.family as u32);
            let va = c.a_verdict.unwrap_or(false) as u32 + c.a_verdict.is_some() as u32;
            let vb = c.b_verdict.unwrap_or(false) as u32 + c.b_verdict.is_some() as u32;
            h = h.wrapping_mul(31).wrapping_add(va * 4 + vb);
            let bytes = c.title.as_bytes();
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
    // 1) 结论冲突（甲绿乙红）必被互认判定抓出
    let mut c = CapabilityJoint::new();
    let _ = c.register(1, FAM_VFS, "冲突样本");
    let _ = c.a_sign(1, true);
    let _ = c.b_sign(1, false);
    got[0] = c.mutual(1) == Ok(false) && !c.conflicts().is_empty();
    // 2) 非法族号必拒
    got[1] = c.register(2, 9, "坏族").err() == Some(FC_BADFAM);
    // 3) 未登记即签必拒
    got[2] = c.a_sign(99, true).err() == Some(FC_NOCRIT)
        && c.b_sign(99, true).err() == Some(FC_NOCRIT);
    // 4) 已签再签必拒
    let mut d = CapabilityJoint::new();
    let _ = d.register(5, FAM_FILE, "重签样本");
    let _ = d.a_sign(5, true);
    got[3] = d.a_sign(5, true).err() == Some(FC_SIGNED)
        && d.b_sign(5, false).is_ok();
    got
}

/// 批内判据。
pub fn run_unx10304_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10304");

    // J1 正路径：三族各两条，双侧同裁互认
    let mut j = CapabilityJoint::new();
    let _ = j.register(1, FAM_VFS, "VFS 挂载判据");
    let _ = j.register(2, FAM_VFS, "VFS 卸载判据");
    let _ = j.register(3, FAM_PATH, "路径解析判据");
    let _ = j.register(4, FAM_PATH, "路径越界判据");
    let _ = j.register(5, FAM_FILE, "文件读写判据");
    let _ = j.register(6, FAM_FILE, "文件关闭判据");
    let mut ok = true;
    let mut n = 1u32;
    while n <= 6 {
        if j.a_sign(n, true).is_err() || j.b_sign(n, true).is_err() {
            ok = false;
        }
        n += 1;
    }
    s.add("J1-三族互认", ok && j.all_covered(), "六条判据双签同裁且三族全覆盖");

    // J2 逐条互认裁断为真
    let j2 = {
        let mut ok = true;
        let mut n = 1u32;
        while n <= 6 {
            if j.mutual(n) != Ok(true) {
                ok = false;
            }
            n += 1;
        }
        ok
    };
    s.add("J2-逐条互认", j2, "六条逐条裁互认为真");

    // J3 注入检出：结论冲突 + 非法族
    let inj = inject_four();
    s.add("J3-冲突坏族检出", inj[0] && inj[1], "甲乙异裁必抓且坏族必拒");

    // J4 注入检出：未登记即签 + 重签
    s.add("J4-越签检出", inj[2] && inj[3], "未登记即签与重签必抓");

    // J5 缺额裁断：VFS 族缺一条即族覆盖失败
    let j5 = {
        let mut c = CapabilityJoint::new();
        let _ = c.register(10, FAM_VFS, "孤条");
        !c.family_covered(FAM_VFS)
    };
    s.add("J5-族缺额", j5, "族内不足锚数即不覆盖");

    // J6 未双签不可裁互认
    let j6 = {
        let mut c = CapabilityJoint::new();
        let _ = c.register(11, FAM_PATH, "半签");
        let _ = c.a_sign(11, true);
        c.mutual(11).err() == Some(FC_UNSIGNED)
    };
    s.add("J6-半签拒裁", j6, "仅单侧裁决不得裁互认");

    // J7 冲突清单可枚举
    let j7 = {
        let mut c = CapabilityJoint::new();
        let _ = c.register(20, FAM_FILE, "冲一");
        let _ = c.register(21, FAM_FILE, "冲二");
        let _ = c.a_sign(20, true);
        let _ = c.b_sign(20, false);
        let _ = c.a_sign(21, false);
        let _ = c.b_sign(21, true);
        c.conflicts() == [20u32, 21u32]
    };
    s.add("J7-冲突枚举", j7, "两条冲突按登记序枚举");

    // J8 容量边界
    let j8 = {
        let mut c = CapabilityJoint::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= CAP as u32 {
            if c.register(n, FAM_VFS, "x").is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.register(999, FAM_VFS, "over").err() == Some(FC_CAP)
    };
    s.add("J8-容量边界", j8, "超容必拒且原册不损");

    // J9 负路径：空题拒
    let j9 = CapabilityJoint::new().register(1, FAM_VFS, "").err() == Some(FC_EMPTY);
    s.add("J9-空题拒收", j9, "判据题空必拒");

    // J10 重号拒
    let j10 = {
        let mut c = CapabilityJoint::new();
        let _ = c.register(1, FAM_VFS, "甲");
        c.register(1, FAM_PATH, "乙").err() == Some(FC_DUP)
    };
    s.add("J10-重号拒收", j10, "判据号跨族重号仍拒");

    // J11 指纹稳定且随签变化
    let j11 = {
        let mut c = CapabilityJoint::new();
        let _ = c.register(30, FAM_VFS, "指");
        let f0 = c.fold();
        let _ = c.a_sign(30, true);
        let f1 = c.fold();
        let _ = c.b_sign(30, true);
        f0 != f1 && c.fold() != f1 && c.fold() == c.fold()
    };
    s.add("J11-指纹随签", j11, "签一步指纹进一步且终态稳定");

    // J12 动态明细：互认计数落账
    let mc = j.crits.iter().filter(|c| c.both_signed() && !c.conflict()).count();
    s.add_dyn("J12-互认计数", mc == 6, &format!("mutual={}", mc));

    // J13 动态明细：冲突零清单
    let cf = j.conflicts().len();
    s.add_dyn("J13-冲突零明细", cf == 0, &format!("conflicts={}", cf));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10304_joint_flow() {
        let mut j = CapabilityJoint::new();
        assert!(j.register(1, FAM_VFS, "a").is_ok());
        assert!(j.register(1, FAM_PATH, "dup").err() == Some(FC_DUP));
        assert!(j.register(2, 7, "bad").err() == Some(FC_BADFAM));
        assert!(j.a_sign(9, true).err() == Some(FC_NOCRIT));
        assert!(j.a_sign(1, true).is_ok());
        assert!(j.a_sign(1, false).err() == Some(FC_SIGNED));
        assert!(j.mutual(1).err() == Some(FC_UNSIGNED));
        assert!(j.b_sign(1, false).is_ok());
        assert_eq!(j.mutual(1), Ok(false));
        assert!(!j.conflicts().is_empty());
    }

    #[test]
    fn unx10304_checks_green() {
        let s = run_unx10304_checks();
        assert!(s.all_passed(), "UNX-F10304 判据未全绿");
    }
}
