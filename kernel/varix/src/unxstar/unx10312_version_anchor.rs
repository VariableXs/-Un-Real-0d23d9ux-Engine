//! UNX-F10312 联签面版本锚（联签绑双方版本）（0x1b180 段）
//!
//! 锚点：UNX-F10312-J1 版本锚绑定——联签主体逐条绑甲乙双方版本，
//! 主版本错位即拒绑、次版本漂移即不对齐，全主体对齐才成锚。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：锚册（主体名→甲(maj,min)/乙(maj,min) 双版登记，主体唯一）+
//! 对齐核（全对齐=双版全等；兼容=仅主版本等）+ 成锚核（全主体对齐
//! 才出锚戳）+ 注入核（主版本错位/次版本漂移/零版本/主体重号四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b180 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b180;
pub const VA_EMPTY: u32 = SEG_BASE + 0x01; // 主体名空
pub const VA_ZERO: u32 = SEG_BASE + 0x02; // 版本号为零
pub const VA_DUP: u32 = SEG_BASE + 0x03; // 主体重号
pub const VA_CAP: u32 = SEG_BASE + 0x04; // 锚册超容
pub const VA_BOUND: u32 = SEG_BASE + 0x05; // 已成锚再绑

/// 锚册定容。
pub const CAP: usize = 12;
/// 主体数锚。
pub const SUBJECTS_NEEDED: usize = 5;

/// 单条版本锚记录。
#[derive(Clone)]
pub struct AnchorRecord {
    pub subject: String,
    pub a_maj: u32,
    pub a_min: u32,
    pub b_maj: u32,
    pub b_min: u32,
}

impl AnchorRecord {
    /// 全对齐：双版全等。
    pub fn aligned(&self) -> bool {
        self.a_maj == self.b_maj && self.a_min == self.b_min
    }

    /// 兼容：仅主版本相等。
    pub fn compatible(&self) -> bool {
        self.a_maj == self.b_maj
    }

    /// 次版本漂移：主等次不等。
    pub fn minor_drift(&self) -> bool {
        self.compatible() && !self.aligned()
    }

    /// 主版本错位。
    pub fn major_mismatch(&self) -> bool {
        self.a_maj != self.b_maj
    }

    pub fn ok(&self) -> bool {
        !self.subject.is_empty()
            && self.a_maj > 0 && self.a_min > 0
            && self.b_maj > 0 && self.b_min > 0
    }
}

/// 联签面版本锚册。
pub struct VersionJoint {
    pub records: Vec<AnchorRecord>,
    pub anchored: bool,
}

impl VersionJoint {
    pub fn new() -> VersionJoint {
        VersionJoint { records: Vec::new(), anchored: false }
    }

    fn pos(&self, subject: &str) -> Option<usize> {
        let mut k = 0usize;
        while k < self.records.len() {
            if self.records[k].subject == subject {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 绑定主体双版：空名、零版、重号、超容、已成锚均拒。
    pub fn bind(&mut self, subject: &str, am: u32, ai: u32, bm: u32, bi: u32) -> Result<(), u32> {
        if self.anchored {
            return Err(VA_BOUND);
        }
        if subject.is_empty() {
            return Err(VA_EMPTY);
        }
        if am == 0 || ai == 0 || bm == 0 || bi == 0 {
            return Err(VA_ZERO);
        }
        if self.pos(subject).is_some() {
            return Err(VA_DUP);
        }
        if self.records.len() >= CAP {
            return Err(VA_CAP);
        }
        self.records.push(AnchorRecord {
            subject: subject.to_string(),
            a_maj: am,
            a_min: ai,
            b_maj: bm,
            b_min: bi,
        });
        Ok(())
    }

    /// 成锚：全主体对齐才许出锚戳。
    pub fn anchor(&mut self) -> Result<u32, u32> {
        if self.anchored {
            return Err(VA_BOUND);
        }
        if self.records.len() < SUBJECTS_NEEDED || !self.all_aligned() {
            return Err(VA_ZERO);
        }
        self.anchored = true;
        Ok(self.stamp())
    }

    /// 全主体对齐裁断。
    pub fn all_aligned(&self) -> bool {
        self.records.iter().all(|r| r.aligned())
    }

    /// 主体 → 双版记录（未知主体即拒）。
    pub fn version_of(&self, subject: &str) -> Result<(u32, u32, u32, u32), u32> {
        match self.pos(subject) {
            None => Err(VA_ZERO),
            Some(k) => {
                let r = &self.records[k];
                Ok((r.a_maj, r.a_min, r.b_maj, r.b_min))
            }
        }
    }

    /// 主版本错位清单。
    pub fn mismatched(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.records.len() {
            if self.records[k].major_mismatch() {
                v.push(k as u32);
            }
            k += 1;
        }
        v
    }

    /// 次版本漂移清单。
    pub fn drifted(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.records.len() {
            if self.records[k].minor_drift() {
                v.push(k as u32);
            }
            k += 1;
        }
        v
    }

    /// 锚戳：逐主体版本字节折叠。
    pub fn stamp(&self) -> u32 {
        let mut h: u32 = 0x1b1800;
        h = h.wrapping_mul(31).wrapping_add(self.records.len() as u32);
        let mut k = 0usize;
        while k < self.records.len() {
            let r = &self.records[k];
            h = h.wrapping_mul(31).wrapping_add(r.a_maj);
            h = h.wrapping_mul(31).wrapping_add(r.a_min);
            h = h.wrapping_mul(31).wrapping_add(r.b_maj);
            h = h.wrapping_mul(31).wrapping_add(r.b_min);
            let bytes = r.subject.as_bytes();
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
    let mut v = VersionJoint::new();
    // 1) 主版本错位必入错位清单
    let _ = v.bind("错位主体", 2, 1, 3, 1);
    got[0] = v.mismatched() == [0u32] && !v.records[0].compatible();
    // 2) 次版本漂移（主等次不等）必入漂移清单且不算全对齐
    let mut w = VersionJoint::new();
    let _ = w.bind("漂移主体", 2, 1, 2, 2);
    got[1] = w.records[0].minor_drift() && w.drifted() == [0u32] && !w.all_aligned();
    // 3) 零版本必拒
    got[2] = w.bind("零版", 0, 1, 1, 1).err() == Some(VA_ZERO)
        && w.bind("零次", 1, 0, 1, 1).err() == Some(VA_ZERO);
    // 4) 主体重号必拒
    let mut d = VersionJoint::new();
    let _ = d.bind("dup", 1, 1, 1, 1);
    got[3] = d.bind("dup", 1, 2, 1, 2).err() == Some(VA_DUP);
    got
}

/// 批内判据。
pub fn run_unx10312_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10312");

    // J1 正路径：五主体逐条绑双版并成锚
    let mut v = VersionJoint::new();
    let mut ok = true;
    let mut n = 1u32;
    while n <= SUBJECTS_NEEDED as u32 {
        let sub = format!("主体{}", n);
        if v.bind(&sub, 3, n, 3, n).is_err() {
            ok = false;
        }
        n += 1;
    }
    let stamp = v.anchor();
    s.add("J1-五主体成锚", ok && stamp.is_ok() && v.anchored, "五主体双版全等并出锚戳");

    // J2 记录字段齐备
    let j2 = v.records.iter().all(|r| r.ok());
    s.add("J2-记录字段齐", j2, "主体名非空且四版号非零");

    // J3 注入检出：主版本错位 + 次版本漂移
    let inj = inject_four();
    s.add("J3-错位漂移检出", inj[0] && inj[1], "主错位入错位清单且次漂移入漂移清单");

    // J4 注入检出：零版本 + 主体重号
    s.add("J4-零版重号检出", inj[2] && inj[3], "零版本必拒且主体重号必拒");

    // J5 兼容但不全对齐不算成锚
    let j5 = {
        let mut c = VersionJoint::new();
        let _ = c.bind("主体", 2, 1, 2, 2);
        c.records[0].compatible() && !c.all_aligned()
    };
    s.add("J5-兼容非对齐", j5, "仅主版本等不算全对齐");

    // J6 空主体名拒
    let j6 = VersionJoint::new().bind("", 1, 1, 1, 1).err() == Some(VA_EMPTY);
    s.add("J6-空名拒收", j6, "主体名空必拒");

    // J7 未达锚数不成锚
    let j7 = {
        let mut c = VersionJoint::new();
        let _ = c.bind("孤", 1, 1, 1, 1);
        c.anchor().err() == Some(VA_ZERO)
    };
    s.add("J7-短册不成锚", j7, "主体数不足锚必拒成锚");

    // J8 成锚后再绑必拒
    let j8 = v.bind("迟到", 1, 1, 1, 1).err() == Some(VA_BOUND);
    s.add("J8-成锚拒绑", j8, "成锚后绑定必拒");

    // J9 容量边界
    let j9 = {
        let mut c = VersionJoint::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= CAP as u32 {
            let sub = format!("c{}", n);
            if c.bind(&sub, n, 1, n, 1).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.bind("over", 1, 1, 1, 1).err() == Some(VA_CAP)
    };
    s.add("J9-容量边界", j9, "超容必拒且原册不损");

    // J10 锚戳稳定
    let j10 = v.stamp() == v.stamp() && v.stamp() != 0;
    s.add("J10-锚戳稳定", j10, "锚戳两次一致且非零");

    // J11 错位漂移双零清单（正册）
    let j11 = v.mismatched().is_empty() && v.drifted().is_empty();
    s.add("J11-双零清单", j11, "正册无错位无漂移");

    // J12 动态明细：锚戳落账
    let st = v.stamp();
    s.add_dyn("J12-锚戳明细", st != 0, &format!("stamp={:#x}", st));

    // J13 动态明细：主体数落账
    let n = v.records.len();
    s.add_dyn("J13-主体数明细", n == SUBJECTS_NEEDED, &format!("subjects={}", n));

    // J14 动态明细：主体双版定向对账
    let sv = v.version_of("主体1");
    let j14 = sv == Ok((3, 1, 3, 1)) && v.version_of("幽灵").err() == Some(VA_ZERO);
    s.add_dyn("J14-定向双版", j14, &format!("主体一双版 {:?}", sv));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10312_anchor_flow() {
        let mut v = VersionJoint::new();
        assert!(v.bind("s1", 1, 1, 1, 1).is_ok());
        assert!(v.bind("s1", 1, 2, 1, 2).err() == Some(VA_DUP));
        assert!(v.bind("", 1, 1, 1, 1).err() == Some(VA_EMPTY));
        assert!(v.bind("z", 0, 1, 1, 1).err() == Some(VA_ZERO));
        assert!(v.bind("s2", 1, 2, 1, 3).is_ok());
        assert!(v.records[1].minor_drift());
        assert!(!v.all_aligned());
        v.records[1].b_min = 2;
        assert!(v.all_aligned());
        // 主体数不足锚：不成锚且未锁定
        assert!(v.anchor().err() == Some(VA_ZERO));
        let names = ["s3", "s4", "s5"];
        let mut k = 0usize;
        while k < names.len() {
            let m = (k + 3) as u32;
            assert!(v.bind(names[k], m, 1, m, 1).is_ok());
            k += 1;
        }
        assert!(v.anchor().is_ok());
        assert!(v.bind("late", 1, 1, 1, 1).err() == Some(VA_BOUND));
        assert!(v.anchor().err() == Some(VA_BOUND));
    }

    #[test]
    fn unx10312_checks_green() {
        let s = run_unx10312_checks();
        assert!(s.all_passed(), "UNX-F10312 判据未全绿");
    }
}
