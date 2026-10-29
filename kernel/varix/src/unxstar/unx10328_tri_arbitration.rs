//! UNX-F10328 异议仲裁（双签）（0x1b380 段）
//!
//! 锚点：UNX-F10328-J1 三面双签仲裁——每件异议先定裁断（维持/驳回），
//! 再由三面中至少两个不同面会签，裁断与双面会签齐备才算该件裁结，
//! 全部异议裁结才算仲裁账清。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：异议账（件=异议号+案由+裁断 Option+三面会签位）+ 裁断核
//! （裁断只许一次、坏裁断码即拒）+ 会签核（同面重签即拒、异面满两
//! 方裁结）+ 注入核（零号/重件/未知件/坏裁断四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b380 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b380;
pub const TA_ZERO: u32 = SEG_BASE + 0x01; // 异议号零
pub const TA_DUP: u32 = SEG_BASE + 0x02; // 异议号重号
pub const TA_NOOBJ: u32 = SEG_BASE + 0x03; // 未知异议
pub const TA_RERULE: u32 = SEG_BASE + 0x04; // 已裁再裁
pub const TA_RECO: u32 = SEG_BASE + 0x05; // 同面重签
pub const TA_VERDICT: u32 = SEG_BASE + 0x06; // 坏裁断码
pub const TA_CAP: u32 = SEG_BASE + 0x07; // 账超容
pub const TA_EMPTY: u32 = SEG_BASE + 0x08; // 案由空

/// 三面数（开区间）。
pub const FACE_N: u8 = 3;
/// 裁结所需最少异面会签数。
pub const COSIGN_NEED: usize = 2;
/// 账定容。
pub const CAP: usize = 12;

/// 裁断码。
pub const VERDICT_UPHOLD: u8 = 1;
pub const VERDICT_REJECT: u8 = 2;

/// 仲裁异议件。
#[derive(Clone)]
pub struct Objection {
    pub oid: u32,
    pub subject: String,
    pub verdict: Option<u8>,
    pub cosigns: [bool; 3],
}

impl Objection {
    /// 异面会签数。
    pub fn cosign_count(&self) -> usize {
        let mut n = 0usize;
        let mut f = 0usize;
        while f < FACE_N as usize {
            if self.cosigns[f] {
                n += 1;
            }
            f += 1;
        }
        n
    }

    /// 裁结：裁断已定且异面会签达锚。
    pub fn settled(&self) -> bool {
        self.verdict.is_some() && self.cosign_count() >= COSIGN_NEED
    }
}

/// 三面仲裁账。
pub struct TriArbitration {
    pub objs: Vec<Objection>,
    pub sealed: bool,
}

impl TriArbitration {
    pub fn new() -> TriArbitration {
        TriArbitration { objs: Vec::new(), sealed: false }
    }

    fn pos(&self, oid: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.objs.len() {
            if self.objs[k].oid == oid {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 立件。
    pub fn file(&mut self, oid: u32, subject: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(TA_CAP);
        }
        if oid == 0 {
            return Err(TA_ZERO);
        }
        if self.pos(oid).is_some() {
            return Err(TA_DUP);
        }
        if subject.is_empty() {
            return Err(TA_EMPTY);
        }
        if self.objs.len() >= CAP {
            return Err(TA_CAP);
        }
        self.objs.push(Objection {
            oid,
            subject: subject.to_string(),
            verdict: None,
            cosigns: [false; 3],
        });
        Ok(())
    }

    /// 定裁断。
    pub fn rule(&mut self, oid: u32, verdict: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(TA_CAP);
        }
        if verdict != VERDICT_UPHOLD && verdict != VERDICT_REJECT {
            return Err(TA_VERDICT);
        }
        match self.pos(oid) {
            None => Err(TA_NOOBJ),
            Some(p) => {
                if self.objs[p].verdict.is_some() {
                    return Err(TA_RERULE);
                }
                self.objs[p].verdict = Some(verdict);
                Ok(())
            }
        }
    }

    /// 面会签。
    pub fn cosign(&mut self, oid: u32, face: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(TA_CAP);
        }
        if face >= FACE_N {
            return Err(TA_VERDICT);
        }
        match self.pos(oid) {
            None => Err(TA_NOOBJ),
            Some(p) => {
                if self.objs[p].cosigns[face as usize] {
                    return Err(TA_RECO);
                }
                self.objs[p].cosigns[face as usize] = true;
                Ok(())
            }
        }
    }

    /// 未裁结清单。
    pub fn unruled(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.objs.len() {
            if !self.objs[k].settled() {
                v.push(self.objs[k].oid);
            }
            k += 1;
        }
        v
    }

    /// 仲裁账清：全部异议裁结。
    pub fn all_settled(&self) -> bool {
        !self.objs.is_empty() && self.unruled().is_empty()
    }

    /// 裁断分布（维持/驳回件数）。
    pub fn verdict_counts(&self) -> (usize, usize) {
        let mut up = 0usize;
        let mut rj = 0usize;
        let mut k = 0usize;
        while k < self.objs.len() {
            match self.objs[k].verdict {
                Some(VERDICT_UPHOLD) => up += 1,
                Some(VERDICT_REJECT) => rj += 1,
                _ => {}
            }
            k += 1;
        }
        (up, rj)
    }

    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(TA_CAP);
        }
        if !self.all_settled() {
            return Err(TA_NOOBJ);
        }
        self.sealed = true;
        Ok(())
    }

    /// 仲裁账指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b3800;
        h = h.wrapping_mul(31).wrapping_add(self.objs.len() as u32);
        let mut k = 0usize;
        while k < self.objs.len() {
            let o = &self.objs[k];
            h = h
                .wrapping_mul(31)
                .wrapping_add(o.oid)
                .wrapping_mul(31)
                .wrapping_add(o.verdict.unwrap_or(0) as u32)
                .wrapping_mul(31)
                .wrapping_add(o.cosign_count() as u32);
            let sb = o.subject.as_bytes();
            let mut i = 0usize;
            while i < sb.len() {
                h = h.wrapping_mul(31).wrapping_add(sb[i] as u32);
                i += 1;
            }
            k += 1;
        }
        h = h.wrapping_mul(31).wrapping_add(self.sealed as u32);
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut a = TriArbitration::new();
    // 1) 零号与坏裁断码必拒
    let _ = a.file(1, "时序异议");
    got[0] = a.file(0, "零件").err() == Some(TA_ZERO)
        && a.rule(1, 7).err() == Some(TA_VERDICT);
    // 2) 重件必拒
    got[1] = a.file(1, "重件").err() == Some(TA_DUP);
    // 3) 未知件裁断与会签必拒
    got[2] = a.rule(99, VERDICT_UPHOLD).err() == Some(TA_NOOBJ)
        && a.cosign(99, 0).err() == Some(TA_NOOBJ);
    // 4) 同面重签必拒
    let _ = a.cosign(1, 0);
    got[3] = a.cosign(1, 0).err() == Some(TA_RECO);
    got
}

/// 批内判据。
pub fn run_unx10328_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10328");

    // J1 正路径：四件异议裁断双面会签全裁结
    let mut a = TriArbitration::new();
    let mut ok = true;
    let mut n = 1u32;
    while n <= 4u32 {
        let sub = "异议件";
        if a.file(n, sub).is_err() {
            ok = false;
        }
        n += 1;
    }
    n = 1;
    while n <= 4u32 {
        let v = if n % 2 == 0 { VERDICT_REJECT } else { VERDICT_UPHOLD };
        if a.rule(n, v).is_err() {
            ok = false;
        }
        n += 1;
    }
    n = 1;
    while n <= 4u32 {
        let f0 = (n % 3) as u8;
        let f1 = ((n + 1) % 3) as u8;
        if a.cosign(n, f0).is_err() || a.cosign(n, f1).is_err() {
            ok = false;
        }
        n += 1;
    }
    let sealed = a.seal();
    s.add(
        "J1-双签仲裁",
        ok && a.all_settled() && sealed.is_ok(),
        "四件异议裁断加两异面会签全裁结并封账",
    );

    // J2 裁断分布对平（两维持两驳回）
    let (up, rj) = a.verdict_counts();
    s.add("J2-裁断分布", up == 2 && rj == 2, "维持两件驳回两件");

    // J3 注入检出：零号坏裁断 + 重件
    let inj = inject_four();
    s.add("J3-零号重件检出", inj[0] && inj[1], "零号与坏裁断码及重件必拒");

    // J4 注入检出：未知件 + 同面重签
    s.add("J4-未知重签检出", inj[2] && inj[3], "未知件与会签同面重签必拒");

    // J5 单面会签不成裁结
    let j5 = {
        let mut t = TriArbitration::new();
        let _ = t.file(1, "单签件");
        let _ = t.rule(1, VERDICT_UPHOLD);
        let _ = t.cosign(1, 2);
        !t.objs[0].settled() && t.unruled().contains(&1)
    };
    s.add("J5-单签不成", j5, "仅一面会签不裁结");

    // J6 缺裁断不成裁结（三面会签但无裁断）
    let j6 = {
        let mut t = TriArbitration::new();
        let _ = t.file(1, "无裁件");
        let _ = t.cosign(1, 0);
        let _ = t.cosign(1, 1);
        !t.objs[0].settled()
    };
    s.add("J6-缺裁不成", j6, "无裁断即便三面会签也不裁结");

    // J7 重裁断必拒
    let j7 = {
        let mut t = TriArbitration::new();
        let _ = t.file(1, "重裁件");
        let _ = t.rule(1, VERDICT_UPHOLD);
        t.rule(1, VERDICT_REJECT).err() == Some(TA_RERULE)
    };
    s.add("J7-重裁拒收", j7, "裁断只许一次");

    // J8 坏面号会签必拒
    let j8 = {
        let mut t = TriArbitration::new();
        let _ = t.file(1, "面号件");
        t.cosign(1, 3).err() == Some(TA_VERDICT)
    };
    s.add("J8-坏面拒签", j8, "面号越界必拒");

    // J9 未裁结阻断封账
    let j9 = {
        let mut t = TriArbitration::new();
        let _ = t.file(1, "挂件");
        t.seal().err() == Some(TA_NOOBJ)
    };
    s.add("J9-挂件拒封", j9, "存在未裁结件即不得封账");

    // J10 容量边界
    let j10 = {
        let mut t = TriArbitration::new();
        let mut n = 1u32;
        let mut filled = true;
        while n <= CAP as u32 {
            let sub = "容件";
            if t.file(n, sub).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && t.file(999, "超").err() == Some(TA_CAP)
    };
    s.add("J10-容量边界", j10, "超容必拒且原账不损");

    // J11 指纹随会签推进
    let j11 = {
        let mut t = TriArbitration::new();
        let _ = t.file(1, "指件");
        let f0 = t.fold();
        let _ = t.cosign(1, 0);
        let f1 = t.fold();
        let _ = t.cosign(1, 1);
        let _ = t.rule(1, VERDICT_REJECT);
        let f3 = t.fold();
        f0 != f1 && f1 != f3 && t.fold() == f3
    };
    s.add("J11-指纹推进", j11, "每会签一步指纹一步且终态稳定");

    // J12 动态明细：未裁结计数
    let un = a.unruled().len();
    s.add_dyn("J12-未裁结数", un == 0, &format!("unruled={}", un));

    // J13 动态明细：仲裁账指纹落账
    let fp = a.fold();
    s.add_dyn("J13-仲裁指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10328_arbitration_flow() {
        let mut a = TriArbitration::new();
        assert!(a.file(0, "零").err() == Some(TA_ZERO));
        assert!(a.file(1, "").err() == Some(TA_EMPTY));
        assert!(a.file(1, "甲件").is_ok());
        assert!(a.file(1, "重").err() == Some(TA_DUP));
        assert!(a.rule(99, VERDICT_UPHOLD).err() == Some(TA_NOOBJ));
        assert!(a.rule(1, 9).err() == Some(TA_VERDICT));
        assert!(a.rule(1, VERDICT_UPHOLD).is_ok());
        assert!(a.rule(1, VERDICT_REJECT).err() == Some(TA_RERULE));
        assert!(a.cosign(1, 3).err() == Some(TA_VERDICT));
        assert!(a.cosign(1, 0).is_ok());
        assert!(a.cosign(1, 0).err() == Some(TA_RECO));
        assert!(!a.objs[0].settled());
        assert!(a.cosign(1, 2).is_ok());
        assert!(a.objs[0].settled());
        assert!(a.seal().is_ok());
        assert!(a.file(2, "封后").err() == Some(TA_CAP));
    }

    #[test]
    fn unx10328_checks_green() {
        let s = run_unx10328_checks();
        assert!(s.all_passed(), "UNX-F10328 判据未全绿");
    }
}
