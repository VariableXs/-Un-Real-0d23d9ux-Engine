//! UNX-F10310 联签异议仲裁（双签仲裁留痕）（0x1b140 段）
//!
//! 锚点：UNX-F10310-J1 仲裁留痕——异议逐条裁决且甲乙双签落档，单签
//! 或漏裁即留痕不闭。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：异议册（异议号+涉判据+理由+提起方登记）+ 裁决账（裁决值
//! 独立落账，重裁即拒）+ 双签账（甲乙两方各签一次，同方双签即拒）+
//! 留痕闭账核（裁+签双账齐备）+ 注入核（同方双签/漏裁/重复双签/
//! 侧别非法四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b140 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b140;
pub const AR_DUPOID: u32 = SEG_BASE + 0x01; // 异议号重号
pub const AR_EMPTY: u32 = SEG_BASE + 0x02; // 理由空
pub const AR_BADBY: u32 = SEG_BASE + 0x03; // 提起方非法
pub const AR_NOOID: u32 = SEG_BASE + 0x04; // 未知异议
pub const AR_RERULED: u32 = SEG_BASE + 0x05; // 已裁再裁
pub const AR_SAMESIGN: u32 = SEG_BASE + 0x06; // 同方双签
pub const AR_RECOSIGN: u32 = SEG_BASE + 0x07; // 重复双签

/// 提起方：1=甲 2=乙。
pub const BY_A: u8 = 1;
pub const BY_B: u8 = 2;
/// 裁决值：1=支持异议 2=驳回异议。
pub const VERDICT_UPHOLD: u8 = 1;
pub const VERDICT_REJECT: u8 = 2;

/// 异议册定容。
pub const CAP: usize = 12;
/// 异议数锚。
pub const OBJ_NEEDED: usize = 4;

/// 单条异议。
#[derive(Clone)]
pub struct Objection {
    pub oid: u32,
    pub ref_cid: u32,
    pub reason: String,
    pub by: u8,
}

/// 联签异议仲裁账。
pub struct Arbitration {
    pub objections: Vec<Objection>,
    pub verdicts: Vec<(u32, u8)>,
    pub signs: Vec<(u32, u8, u8)>,
}

impl Arbitration {
    pub fn new() -> Arbitration {
        Arbitration { objections: Vec::new(), verdicts: Vec::new(), signs: Vec::new() }
    }

    fn pos(&self, oid: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.objections.len() {
            if self.objections[k].oid == oid {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 提异议：重号、空理由、侧别非法、超容均拒。
    pub fn file(&mut self, oid: u32, ref_cid: u32, reason: &str, by: u8) -> Result<(), u32> {
        if oid == 0 || self.pos(oid).is_some() {
            return Err(AR_DUPOID);
        }
        if reason.is_empty() {
            return Err(AR_EMPTY);
        }
        if by != BY_A && by != BY_B {
            return Err(AR_BADBY);
        }
        if self.objections.len() >= CAP {
            return Err(AR_DUPOID);
        }
        self.objections.push(Objection {
            oid,
            ref_cid,
            reason: reason.to_string(),
            by,
        });
        Ok(())
    }

    /// 裁决：未知异议与重裁均拒。
    pub fn rule(&mut self, oid: u32, verdict: u8) -> Result<(), u32> {
        if self.pos(oid).is_none() {
            return Err(AR_NOOID);
        }
        if verdict != VERDICT_UPHOLD && verdict != VERDICT_REJECT {
            return Err(AR_RERULED);
        }
        if self.verdicts.iter().any(|v| v.0 == oid) {
            return Err(AR_RERULED);
        }
        self.verdicts.push((oid, verdict));
        Ok(())
    }

    /// 双签：甲乙各签一次，同方双签或重复双签即拒。
    pub fn cosign(&mut self, oid: u32, s1: u8, s2: u8) -> Result<(), u32> {
        if self.pos(oid).is_none() {
            return Err(AR_NOOID);
        }
        if s1 != BY_A && s1 != BY_B {
            return Err(AR_BADBY);
        }
        if s2 != BY_A && s2 != BY_B {
            return Err(AR_BADBY);
        }
        if s1 == s2 {
            return Err(AR_SAMESIGN);
        }
        if self.signs.iter().any(|s| s.0 == oid) {
            return Err(AR_RECOSIGN);
        }
        self.signs.push((oid, s1, s2));
        Ok(())
    }

    /// 单条留痕齐备：已裁且已双签。
    pub fn trailed(&self, oid: u32) -> bool {
        self.verdicts.iter().any(|v| v.0 == oid) && self.signs.iter().any(|s| s.0 == oid)
    }

    /// 留痕闭账：全部异议裁+签齐备。
    pub fn trail_closed(&self) -> bool {
        self.objections.len() >= OBJ_NEEDED
            && self.objections.iter().all(|o| self.trailed(o.oid))
    }

    /// 漏裁异议清单。
    pub fn unruled(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.objections.len() {
            let oid = self.objections[k].oid;
            if !self.verdicts.iter().any(|v| v.0 == oid) {
                v.push(oid);
            }
            k += 1;
        }
        v
    }

    /// 异议号 → 裁决值（未裁或未知均拒）。
    pub fn verdict_of(&self, oid: u32) -> Result<u8, u32> {
        let mut k = 0usize;
        while k < self.verdicts.len() {
            if self.verdicts[k].0 == oid {
                return Ok(self.verdicts[k].1);
            }
            k += 1;
        }
        Err(AR_NOOID)
    }

    /// 异议号 → 提起方（未知异议即拒）。
    pub fn filed_by(&self, oid: u32) -> Result<u8, u32> {
        match self.pos(oid) {
            None => Err(AR_NOOID),
            Some(k) => Ok(self.objections[k].by),
        }
    }

    /// 仲裁账指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b1400;
        h = h.wrapping_mul(31).wrapping_add(self.objections.len() as u32);
        let mut k = 0usize;
        while k < self.objections.len() {
            let o = &self.objections[k];
            h = h.wrapping_mul(31).wrapping_add(o.oid);
            h = h.wrapping_mul(31).wrapping_add(o.ref_cid);
            h = h.wrapping_mul(31).wrapping_add(o.by as u32);
            let bytes = o.reason.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = h.wrapping_mul(31).wrapping_add(bytes[b] as u32);
                b += 1;
            }
            k += 1;
        }
        h = h.wrapping_mul(31).wrapping_add(self.verdicts.len() as u32);
        h = h.wrapping_mul(31).wrapping_add(self.signs.len() as u32);
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut a = Arbitration::new();
    let _ = a.file(1, 10, "样本异议", BY_A);
    let _ = a.rule(1, VERDICT_UPHOLD);
    // 1) 同方双签必拒
    got[0] = a.cosign(1, BY_A, BY_A).err() == Some(AR_SAMESIGN);
    // 2) 重复双签必拒（先补齐合法双签）
    let _ = a.cosign(1, BY_A, BY_B);
    got[1] = a.cosign(1, BY_B, BY_A).err() == Some(AR_RECOSIGN);
    // 3) 漏裁异议堵闭账
    let _ = a.file(2, 11, "未裁异议", BY_B);
    got[2] = !a.trail_closed() && a.unruled() == [2u32];
    // 4) 提起方非法必拒
    got[3] = a.file(3, 12, "坏方", 7).err() == Some(AR_BADBY);
    got
}

/// 批内判据。
pub fn run_unx10310_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10310");

    // J1 正路径：四异议逐条裁决+双签留痕闭账
    let mut a = Arbitration::new();
    let mut ok = true;
    let mut n = 1u32;
    while n <= OBJ_NEEDED as u32 {
        if a.file(n, n * 10, "联签异议", if n % 2 == 0 { BY_B } else { BY_A }).is_err() {
            ok = false;
        }
        n += 1;
    }
    n = 1u32;
    while n <= OBJ_NEEDED as u32 {
        let v = if n % 2 == 0 { VERDICT_REJECT } else { VERDICT_UPHOLD };
        if a.rule(n, v).is_err() || a.cosign(n, BY_A, BY_B).is_err() {
            ok = false;
        }
        n += 1;
    }
    s.add("J1-四异议留痕闭账", ok && a.trail_closed(), "裁+签双账齐备且闭账成立");

    // J2 逐条留痕核
    let j2 = {
        let mut ok = true;
        let mut n = 1u32;
        while n <= OBJ_NEEDED as u32 {
            if !a.trailed(n) {
                ok = false;
            }
            n += 1;
        }
        ok
    };
    s.add("J2-逐条留痕", j2, "四条逐条裁签齐备");

    // J3 注入检出：同方双签 + 重复双签
    let inj = inject_four();
    s.add("J3-双签违规检出", inj[0] && inj[1], "同方双签与重复双签必拒");

    // J4 注入检出：漏裁 + 侧别非法
    s.add("J4-漏裁坏方检出", inj[2] && inj[3], "漏裁堵闭账且坏方必拒");

    // J5 重号异议拒
    let j5 = {
        let mut c = Arbitration::new();
        let _ = c.file(1, 1, "甲", BY_A);
        c.file(1, 2, "乙", BY_B).err() == Some(AR_DUPOID)
    };
    s.add("J5-重号拒收", j5, "异议号重号必拒");

    // J6 空理由拒
    let j6 = Arbitration::new().file(1, 1, "", BY_A).err() == Some(AR_EMPTY);
    s.add("J6-空理由拒", j6, "异议理由空必拒");

    // J7 未知异议裁决拒
    let j7 = Arbitration::new().rule(9, VERDICT_UPHOLD).err() == Some(AR_NOOID)
        && Arbitration::new().cosign(9, BY_A, BY_B).err() == Some(AR_NOOID);
    s.add("J7-未知异议拒", j7, "未知异议裁签均拒");

    // J8 重裁拒
    let j8 = {
        let mut c = Arbitration::new();
        let _ = c.file(1, 1, "重裁", BY_A);
        let _ = c.rule(1, VERDICT_UPHOLD);
        c.rule(1, VERDICT_REJECT).err() == Some(AR_RERULED)
    };
    s.add("J8-重裁拒", j8, "已裁再裁必拒");

    // J9 双签次序无关：先乙后甲合法
    let j9 = {
        let mut c = Arbitration::new();
        let _ = c.file(1, 1, "序", BY_B);
        let _ = c.rule(1, VERDICT_REJECT);
        c.cosign(1, BY_B, BY_A).is_ok()
    };
    s.add("J9-双签序无关", j9, "甲乙签名次序不影响留痕成立");

    // J10 裁决值非法按重裁面拒
    let j10 = {
        let mut c = Arbitration::new();
        let _ = c.file(1, 1, "值", BY_A);
        c.rule(1, 9).err() == Some(AR_RERULED)
    };
    s.add("J10-裁决值非法", j10, "裁决值出二值域必拒");

    // J11 仲裁账指纹稳定
    let j11 = a.fold() == a.fold() && a.fold() != 0;
    s.add("J11-指纹稳定", j11, "仲裁账折叠指纹两次一致");

    // J12 动态明细：闭账异议数落账
    let tc = a.objections.iter().filter(|o| a.trailed(o.oid)).count();
    s.add_dyn("J12-留痕计数", tc == OBJ_NEEDED, &format!("trailed={}", tc));

    // J13 动态明细：漏裁零清单
    let ur = a.unruled().len();
    s.add_dyn("J13-漏裁零明细", ur == 0, &format!("unruled={}", ur));

    // J14 动态明细：裁决值与提起方定向对账
    let v1 = a.verdict_of(1);
    let b4 = a.filed_by(4);
    let j14 = v1 == Ok(VERDICT_UPHOLD)
        && b4 == Ok(BY_B)
        && a.verdict_of(99).err() == Some(AR_NOOID);
    s.add_dyn(
        "J14-裁签定向",
        j14,
        &format!("异议一裁 {:?} 异议四提起方 {:?}", v1, b4),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10310_arbitration_flow() {
        let mut a = Arbitration::new();
        assert!(a.file(1, 10, "r", BY_A).is_ok());
        assert!(a.file(1, 11, "dup", BY_B).err() == Some(AR_DUPOID));
        assert!(a.file(2, 12, "", BY_A).err() == Some(AR_EMPTY));
        assert!(a.file(3, 13, "bad", 7).err() == Some(AR_BADBY));
        assert!(a.rule(9, VERDICT_UPHOLD).err() == Some(AR_NOOID));
        assert!(a.rule(1, VERDICT_UPHOLD).is_ok());
        assert!(a.rule(1, VERDICT_REJECT).err() == Some(AR_RERULED));
        assert!(a.cosign(1, BY_A, BY_A).err() == Some(AR_SAMESIGN));
        assert!(a.cosign(1, BY_A, BY_B).is_ok());
        assert!(a.cosign(1, BY_B, BY_A).err() == Some(AR_RECOSIGN));
        assert!(a.trailed(1));
        assert!(!a.trail_closed());
    }

    #[test]
    fn unx10310_checks_green() {
        let s = run_unx10310_checks();
        assert!(s.all_passed(), "UNX-F10310 判据未全绿");
    }
}
