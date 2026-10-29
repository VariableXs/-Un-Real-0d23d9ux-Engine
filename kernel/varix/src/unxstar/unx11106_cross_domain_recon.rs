//! UNX-F11106 跨域对账面总核（C2 挂号/C3 释放闸/R-C4-002）（0x1f586 段）
//!
//! 锚点：UNX-F11106-J1 跨域账逐面终核（10/10），承接账零悬空断言（10/10）。
//! 域：CoRun·UNX-C4-B36（跨域账面）。
//!
//! 结构：跨域票册（CrossFace：C2 挂号 → C3 释放闸 → 风险项闭合 → 双向
//! 对平）+ 单票（CrossTicket：挂号位 + 释放位）+ 悬空枚举 + 冻结根 + 注入
//! 核（未挂先释放/重复释放/重号挂号/风险未闭四路必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1f586 独占 0x20）。
pub const SEG_BASE: u32 = 0x1f586;
pub const XD_BADID: u32 = SEG_BASE + 0x01; // 票号为 0
pub const XD_DUP: u32 = SEG_BASE + 0x02; // 票号重号
pub const XD_EMPTY: u32 = SEG_BASE + 0x03; // 域名为空
pub const XD_UNREG: u32 = SEG_BASE + 0x04; // 未挂号先释放
pub const XD_RELEASED: u32 = SEG_BASE + 0x05; // 重复释放
pub const XD_DANGLING: u32 = SEG_BASE + 0x06; // 承接账悬空
pub const XD_RISK: u32 = SEG_BASE + 0x07; // 风险项未闭
pub const XD_FROZEN: u32 = SEG_BASE + 0x08; // 冻结后写账

/// 跨域票数（八张）。
pub const NEEDED: usize = 8;
/// 票册定容。
pub const CAP: usize = 12;
/// 跨域面标（fold 种子）。
pub const XD_TAG: u64 = 0x1f5_8686;
/// 指纹步进。
pub const FP_STEP: u64 = 0x100000001b3;

/// 单票：C2 挂号位 + C3 释放位。
#[derive(Clone, Debug, PartialEq)]
pub struct CrossTicket {
    pub id: u32,
    pub domain: String,
    pub registered: bool,
    pub released: bool,
}

impl CrossTicket {
    /// 双向闭合：既挂号又释放。
    pub fn closed(&self) -> bool {
        self.registered && self.released
    }

    pub fn render(&self) -> String {
        let r = if self.registered { "C2" } else { "--" };
        let l = if self.released { "C3" } else { "--" };
        format!("T{}|{}|{}|{}", self.id, r, l, self.domain)
    }
}

/// 跨域票册：八票 + 风险闭合位 + 冻结根。
#[derive(Clone, Debug)]
pub struct CrossFace {
    pub tickets: Vec<CrossTicket>,
    pub risk_closed: bool,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl CrossFace {
    pub fn new() -> CrossFace {
        CrossFace { tickets: Vec::new(), risk_closed: false, frozen: false, root: None }
    }

    /// C2 挂号：零号/重号/空域/超容皆拒。
    pub fn register(&mut self, id: u32, domain: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(XD_FROZEN);
        }
        if id == 0 {
            return Err(XD_BADID);
        }
        if domain.is_empty() {
            return Err(XD_EMPTY);
        }
        if self.tickets.iter().any(|t| t.id == id) {
            return Err(XD_DUP);
        }
        if self.tickets.len() >= CAP {
            return Err(XD_DANGLING);
        }
        self.tickets.push(CrossTicket {
            id,
            domain: domain.to_string(),
            registered: true,
            released: false,
        });
        Ok(())
    }

    fn find(&mut self, id: u32) -> Option<&mut CrossTicket> {
        let mut k = 0usize;
        while k < self.tickets.len() {
            if self.tickets[k].id == id {
                return Some(&mut self.tickets[k]);
            }
            k += 1;
        }
        None
    }

    /// C3 释放闸：未挂号拒、重复释放拒。
    pub fn release(&mut self, id: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(XD_FROZEN);
        }
        let t = self.find(id).ok_or(XD_UNREG)?;
        if !t.registered {
            return Err(XD_UNREG);
        }
        if t.released {
            return Err(XD_RELEASED);
        }
        t.released = true;
        Ok(())
    }

    /// 风险项 R-C4-002 闭合位。
    pub fn close_risk(&mut self) -> Result<(), u32> {
        if self.frozen {
            return Err(XD_FROZEN);
        }
        self.risk_closed = true;
        Ok(())
    }

    /// 悬空枚举：已挂号未释放的票号。
    pub fn dangling(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.tickets.len() {
            if !self.tickets[k].released {
                v.push(self.tickets[k].id);
            }
            k += 1;
        }
        v
    }

    /// 双向闭合票数。
    pub fn closed_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.tickets.len() {
            if self.tickets[k].closed() {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 承接连带：每张票都从 C2 走到 C3。
    pub fn two_way(&self) -> bool {
        self.tickets.len() == NEEDED && self.closed_count() == NEEDED && self.dangling().is_empty()
    }

    /// 面级闭合：双向对平且风险项已闭。
    pub fn face_closed(&self) -> bool {
        self.two_way() && self.risk_closed
    }

    /// 冻结并落根。
    pub fn seal(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }

    /// 面级指纹：票号 + 双位 + 域名 + 风险位。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = XD_TAG;
        let mut k = 0usize;
        while k < self.tickets.len() {
            let t = &self.tickets[k];
            h = (h ^ t.id as u64).wrapping_mul(FP_STEP);
            h = (h ^ t.registered as u64).wrapping_mul(FP_STEP);
            h = (h ^ t.released as u64).wrapping_mul(FP_STEP);
            let bytes = t.domain.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(FP_STEP);
                b += 1;
            }
            k += 1;
        }
        h = (h ^ self.risk_closed as u64).wrapping_mul(FP_STEP);
        h
    }
}

/// 域名样本（确定性）。
pub fn domain_name(id: u32) -> String {
    format!("跨域承接域 {}", id)
}

/// 标准跨域面：八票 C2 挂号 → C3 释放 → 风险项闭合。
pub fn build_face() -> (CrossFace, bool) {
    let mut f = CrossFace::new();
    let mut ok = true;
    let mut id = 1u32;
    while id <= NEEDED as u32 {
        if f.register(id, &domain_name(id)).is_err() {
            ok = false;
        }
        if f.release(id).is_err() {
            ok = false;
        }
        id += 1;
    }
    if f.close_risk().is_err() {
        ok = false;
    }
    (f, ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 未挂号先释放必拒
    let mut a = CrossFace::new();
    got[0] = a.release(99).err() == Some(XD_UNREG) && a.dangling().is_empty();
    // 2) 重复释放必拒
    let mut b = CrossFace::new();
    let _ = b.register(1, "重释");
    let first = b.release(1).is_ok();
    got[1] = first && b.release(1).err() == Some(XD_RELEASED);
    // 3) 重号挂号必拒
    let mut c = CrossFace::new();
    let _ = c.register(2, "甲");
    got[2] = c.register(2, "乙").err() == Some(XD_DUP);
    // 4) 风险项未闭则面级闭合为假
    let mut d = CrossFace::new();
    let mut id = 1u32;
    while id <= NEEDED as u32 {
        let _ = d.register(id, &domain_name(id));
        let _ = d.release(id);
        id += 1;
    }
    got[3] = d.two_way() && !d.face_closed();
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11106_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11106");

    let (mut face, built) = build_face();
    // J1 跨域账逐面终核
    s.add(
        "J1-跨域逐面终核",
        built && face.tickets.len() == NEEDED && face.two_way() && face.face_closed(),
        "八票 C2 挂号与 C3 释放双向对平且风险闭合",
    );

    // J2 承接账零悬空
    s.add(
        "J2-承接零悬空",
        face.dangling().is_empty() && face.closed_count() == NEEDED,
        "悬空票号为空且闭合票数足额",
    );

    // J3 未挂号先释放拒
    let j3 = {
        let mut c = CrossFace::new();
        c.release(7).err() == Some(XD_UNREG)
    };
    s.add("J3-未挂拒释放", j3, "未挂号票释放必拒");

    // J4 重复释放拒
    let j4 = {
        let mut c = CrossFace::new();
        let _ = c.register(1, "重释");
        let _ = c.release(1);
        c.release(1).err() == Some(XD_RELEASED)
    };
    s.add("J4-重复释放拒", j4, "同票二次释放必拒");

    // J5 重号挂号拒
    let j5 = {
        let mut c = CrossFace::new();
        let _ = c.register(1, "甲");
        c.register(1, "乙").err() == Some(XD_DUP)
    };
    s.add("J5-重号挂号拒", j5, "同票二次挂号必拒");

    // J6 空域名与零号拒
    let j6 = {
        let mut c = CrossFace::new();
        let a = c.register(1, "").err() == Some(XD_EMPTY);
        let b = c.register(0, "零号").err() == Some(XD_BADID);
        a && b && c.tickets.is_empty()
    };
    s.add("J6-空域零号拒", j6, "空域名与零号挂号两路必拒");

    // J7 风险未闭不判闭合
    let j7 = {
        let mut c = CrossFace::new();
        let mut id = 1u32;
        while id <= NEEDED as u32 {
            let _ = c.register(id, &domain_name(id));
            let _ = c.release(id);
            id += 1;
        }
        c.two_way() && !c.face_closed()
    };
    s.add("J7-风险未闭不判", j7, "风险项 R-C4-002 未闭即面级不闭合");

    // J8 悬空枚举可点名
    let j8 = {
        let mut c = CrossFace::new();
        let _ = c.register(1, "甲");
        let _ = c.register(2, "乙");
        let _ = c.release(1);
        c.dangling().len() == 1 && c.dangling().contains(&2) && !c.two_way()
    };
    s.add("J8-悬空点名", j8, "未释放票可点名且双向对平为假");

    // J9 注入检出：未挂释放 + 重复释放
    let inj = inject_four();
    s.add("J9-未挂重释检出", inj[0] && inj[1], "未挂释放与重复释放两路必抓");

    // J10 注入检出：重号 + 风险未闭
    s.add("J10-重号风险检出", inj[2] && inj[3], "重号挂号与风险未闭两路必抓");

    // J11 冻结拒写
    let root = face.seal();
    let j11 = face.register(9, "追挂").err() == Some(XD_FROZEN)
        && face.release(1).err() == Some(XD_FROZEN)
        && face.close_risk().err() == Some(XD_FROZEN);
    s.add("J11-冻结拒写", j11, "封存后挂号/释放/闭险三路必拒");

    // J12 冻结根稳定
    s.add(
        "J12-冻结根稳定",
        face.root == Some(root) && root == face.fold() && root != 0,
        "跨域面冻结根可重算零漂移",
    );

    // J13 双旗渲染
    let j13 = {
        let (c, _) = build_face();
        c.tickets[0].render().contains("C2")
            && c.tickets[0].render().contains("C3")
            && c.tickets[0].render() != c.tickets[1].render()
    };
    s.add("J13-双旗渲染", j13, "已挂已释票渲染带 C2/C3 双旗");

    // J14 动态明细
    s.add_dyn(
        "J14-跨域明细",
        face.face_closed() && face.fold() == root,
        &format!(
            "closed={} dangling={} risk={} root={:#x}",
            face.closed_count(),
            face.dangling().len(),
            face.risk_closed as u8,
            root
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11106_cross_flow() {
        let (mut f, built) = build_face();
        assert!(built);
        assert!(f.two_way());
        assert!(f.face_closed());
        assert!(f.dangling().is_empty());
        assert!(f.seal() != 0);
        assert!(f.release(1).err() == Some(XD_FROZEN));
    }

    #[test]
    fn unx11106_checks_green() {
        let s = run_unx11106_checks();
        assert!(s.all_passed(), "UNX-F11106 判据未全绿");
    }
}
