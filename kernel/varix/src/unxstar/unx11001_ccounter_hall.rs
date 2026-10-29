//! UNX-F11001 挂号联签场搭建（futex/epoll 族挂点总账）（0x1e020 段）
//!
//! 锚点：UNX-F11001-J1 联签场全链搭建（10/10），挂点总账逐项对平（10/10）。
//! 域：CoRun·UNX-C4-B31（futex/epoll 挂号联签域）。
//!
//! 结构：联签场（futex/epoll 两族挂点各十路定容登记，族标独立）+ 联签对
//! 平器（两族同名挂点逐项配对、值一致才签）+ 挂点总账（登记/对平/枚举
//! 缺口/fold 指纹/冻结全链）+ 注入核（族错配/值偏离/重号/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e020 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e020;
pub const CH_VALUE: u32 = SEG_BASE + 0x01; // 配对值不等
pub const CH_SHORT: u32 = SEG_BASE + 0x02; // 挂点未齐
pub const CH_DUP: u32 = SEG_BASE + 0x03; // 挂点重号
pub const CH_EMPTY: u32 = SEG_BASE + 0x04; // 挂点名空
pub const CH_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后写账
pub const CH_NOHOOK: u32 = SEG_BASE + 0x06; // 配对目标不存在

/// 每族挂点数（10/10）。
pub const HOOKS_PER_FAMILY: usize = 10;
/// 单族定容。
pub const FAMILY_CAP: usize = 12;
/// 联签场域标（fold 种子）。
pub const HALL_TAG: u64 = 0x1e02b31;

/// 族标：futex / epoll 两族。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Futex,
    Epoll,
}

impl Family {
    /// 族标符号（f=E，epoll=P）。
    pub fn mark(&self) -> char {
        match self {
            Family::Futex => 'F',
            Family::Epoll => 'P',
        }
    }
}

/// 单路挂点：族标 + 挂点名 + 对比值 + 联签位。
#[derive(Clone, Debug)]
pub struct Hook {
    pub fam: Family,
    pub name: String,
    pub val: u32,
    pub signed: bool,
}

impl Hook {
    pub fn render(&self) -> String {
        format!("{}|{}|{}", self.fam.mark(), self.name, self.val)
    }
}

/// 挂号联签场：两族挂点登记 → 逐项配对联签 → 总账冻结。
pub struct CosignHall {
    pub hooks: Vec<Hook>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl CosignHall {
    pub fn new() -> CosignHall {
        CosignHall { hooks: Vec::new(), frozen: false, root: None }
    }

    /// 登记挂点：冻结、空名、零值、重号（同族同名）、超容均拒。
    pub fn enroll(&mut self, fam: Family, name: &str, val: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(CH_FROZEN);
        }
        if name.is_empty() {
            return Err(CH_EMPTY);
        }
        if val == 0 {
            return Err(CH_VALUE);
        }
        let dup = {
            let mut d = false;
            let mut k = 0usize;
            while k < self.hooks.len() {
                if self.hooks[k].fam == fam && self.hooks[k].name == name {
                    d = true;
                }
                k += 1;
            }
            d
        };
        if dup {
            return Err(CH_DUP);
        }
        if self.family_len(fam) >= FAMILY_CAP {
            return Err(CH_SHORT);
        }
        self.hooks.push(Hook { fam, name: name.to_string(), val, signed: false });
        Ok(())
    }

    /// 单族在册挂点数。
    pub fn family_len(&self, fam: Family) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.hooks.len() {
            if self.hooks[k].fam == fam {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 联签配对：两族同名挂点值一致则双双翻联签位，否则拒。
    pub fn cosign(&mut self, name: &str) -> Result<bool, u32> {
        if self.frozen {
            return Err(CH_FROZEN);
        }
        let mut fi: i64 = -1;
        let mut pi: i64 = -1;
        let mut k = 0usize;
        while k < self.hooks.len() {
            if self.hooks[k].name == name {
                if self.hooks[k].fam == Family::Futex {
                    fi = k as i64;
                } else {
                    pi = k as i64;
                }
            }
            k += 1;
        }
        if fi < 0 || pi < 0 {
            return Err(CH_NOHOOK);
        }
        let f = fi as usize;
        let p = pi as usize;
        if self.hooks[f].val != self.hooks[p].val {
            return Err(CH_VALUE);
        }
        self.hooks[f].signed = true;
        self.hooks[p].signed = true;
        Ok(true)
    }

    /// 未联签挂点名清单（按登记序）。
    pub fn gaps(&self) -> Vec<String> {
        let mut g: Vec<String> = Vec::new();
        let mut k = 0usize;
        while k < self.hooks.len() {
            if !self.hooks[k].signed {
                g.push(self.hooks[k].name.to_string());
            }
            k += 1;
        }
        g
    }

    /// 总账裁断：两族各十路齐、全数联签。
    pub fn closed(&self) -> bool {
        self.family_len(Family::Futex) == HOOKS_PER_FAMILY
            && self.family_len(Family::Epoll) == HOOKS_PER_FAMILY
            && self.gaps().is_empty()
    }

    /// 链式 fold 指纹（FNV 风格，序敏感，含族标/名/值/联签位）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = HALL_TAG;
        let mut k = 0usize;
        while k < self.hooks.len() {
            let it = &self.hooks[k];
            h = (h ^ match it.fam { Family::Futex => 1u64, Family::Epoll => 2u64 })
                .wrapping_mul(0x100000001b3);
            h = (h ^ it.val as u64).wrapping_mul(0x100000001b3);
            h = (h ^ it.signed as u64).wrapping_mul(0x100000001b3);
            let bytes = it.name.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(0x100000001b3);
                b += 1;
            }
            k += 1;
        }
        h
    }

    /// 冻结：翻冻结位并落冻结根。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// 挂点名确定性样本值（正反两路共用）。
pub fn hall_sample(no: u32) -> u32 {
    no.wrapping_mul(7).wrapping_add(11)
}

/// 挂点名确定性文本。
pub fn hall_name(no: u32) -> String {
    format!("挂点{}", no)
}

/// 全链搭建：两族各十路登记并逐项联签。
pub fn build_hall() -> (CosignHall, bool) {
    let mut hall = CosignHall::new();
    let mut i = 1u32;
    while i <= HOOKS_PER_FAMILY as u32 {
        let v = hall_sample(i);
        let nm = hall_name(i);
        let _ = hall.enroll(Family::Futex, &nm, v);
        let _ = hall.enroll(Family::Epoll, &nm, v);
        i += 1;
    }
    let mut i = 1u32;
    let mut ok = true;
    while i <= HOOKS_PER_FAMILY as u32 {
        if hall.cosign(&hall_name(i)) != Ok(true) {
            ok = false;
        }
        i += 1;
    }
    let hall_ok = ok && hall.closed();
    (hall, hall_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 族错配：只登记一族同名挂点必拒
    let mut a = CosignHall::new();
    let _ = a.enroll(Family::Futex, "错配", 5);
    got[0] = a.cosign("错配").err() == Some(CH_NOHOOK);
    // 2) 值偏离必拒且不翻位
    let mut b = CosignHall::new();
    let _ = b.enroll(Family::Futex, "偏离", 5);
    let _ = b.enroll(Family::Epoll, "偏离", 6);
    got[1] = b.cosign("偏离").err() == Some(CH_VALUE) && b.gaps().len() == 2;
    // 3) 同族重号必拒
    let mut c = CosignHall::new();
    let _ = c.enroll(Family::Futex, "重号", 1);
    got[2] = c.enroll(Family::Futex, "重号", 2).err() == Some(CH_DUP);
    // 4) 冻结后写账必抓
    let (mut d, _) = build_hall();
    d.freeze();
    got[3] = d.enroll(Family::Futex, "迟到", 9).err() == Some(CH_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11001_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11001");

    // J1 联签场全链搭建（10/10）
    let (mut hall, built) = build_hall();
    s.add("J1-联签场全链搭建", built && hall.closed(), "两族各十路登记且逐项联签全绿");

    // J2 挂点总账逐项对平（10/10）：联签位全翻且缺口清空
    s.add("J2-挂点总账对平", hall.gaps().is_empty() && hall.family_len(Family::Futex) == 10 && hall.family_len(Family::Epoll) == 10, "二十路挂点全数联签零缺口");

    // J3 冻结根：fold 稳定且与根一致
    let root = hall.freeze();
    s.add("J3-冻结根", hall.root == Some(root) && hall.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J4 冻结后写账必抓
    let j4 = hall.enroll(Family::Epoll, "迟到", 1).err() == Some(CH_FROZEN);
    s.add("J4-冻结拒写", j4, "冻结后登记必拒");

    // J5 注入检出：族错配 + 值偏离
    let inj = inject_four();
    s.add("J5-错配偏离检出", inj[0] && inj[1], "族错配与值偏离两路必抓");

    // J6 注入检出：重号 + 冻结后写
    s.add("J6-重号冻结检出", inj[2] && inj[3], "同族重号必拒且冻结后写必抓");

    // J7 族标符号可区分
    let j7 = Family::Futex.mark() == 'F' && Family::Epoll.mark() == 'P';
    s.add("J7-族标分旗", j7, "futex/epoll 族标渲染可区分");

    // J8 零值拒收
    let j8 = CosignHall::new().enroll(Family::Futex, "零值", 0).err() == Some(CH_VALUE);
    s.add("J8-零值拒收", j8, "对比值零必拒");

    // J9 空名拒收
    let j9 = CosignHall::new().enroll(Family::Epoll, "", 1).err() == Some(CH_EMPTY);
    s.add("J9-空名拒收", j9, "挂点名空必拒");

    // J10 缺口语义：只联签一路时其余可枚举且 closed 为假
    let j10 = {
        let mut half = CosignHall::new();
        let mut i = 1u32;
        while i <= HOOKS_PER_FAMILY as u32 {
            let v = hall_sample(i);
            let nm = hall_name(i);
            let _ = half.enroll(Family::Futex, &nm, v);
            let _ = half.enroll(Family::Epoll, &nm, v);
            i += 1;
        }
        let _ = half.cosign(&hall_name(1));
        half.gaps().len() == HOOKS_PER_FAMILY * 2 - 2 && !half.closed()
    };
    s.add("J10-缺口枚举", j10, "漏联签可枚举且总账为假");

    // J11 配对目标不存在拒
    let j11 = CosignHall::new().cosign("无此挂点").err() == Some(CH_NOHOOK);
    s.add("J11-目标不存在", j11, "配对不存在的挂点必拒");

    // J12 单族超容拒：futex 族超过 FAMILY_CAP 必拒
    let j12 = {
        let mut c = CosignHall::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= FAMILY_CAP as u32 {
            let nm = format!("超容{}", n);
            if c.enroll(Family::Futex, &nm, n.wrapping_add(1)).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.enroll(Family::Futex, "溢出", 1).err() == Some(CH_SHORT)
    };
    s.add("J12-单族超容", j12, "族内超出定容必拒");

    // J13 未联签不算收口：值齐但联签位未翻
    let j13 = {
        let mut c = CosignHall::new();
        let mut i = 1u32;
        while i <= HOOKS_PER_FAMILY as u32 {
            let v = hall_sample(i);
            let nm = hall_name(i);
            let _ = c.enroll(Family::Futex, &nm, v);
            let _ = c.enroll(Family::Epoll, &nm, v);
            i += 1;
        }
        !c.closed() && c.gaps().len() == HOOKS_PER_FAMILY * 2
    };
    s.add("J13-未翻位不算收", j13, "值齐但未逐项联签仍不算收口");

    // J14 样本确定性：同号两次求值一致且异号互异
    let v3a = hall_sample(3);
    let v3b = hall_sample(3);
    let mut distinct = true;
    let mut i = 1u32;
    while i <= HOOKS_PER_FAMILY as u32 {
        let mut j = i.wrapping_add(1);
        while j <= HOOKS_PER_FAMILY as u32 {
            if hall_sample(i) == hall_sample(j) {
                distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("J14-样本确定性", v3a == v3b && distinct, "样本同入同出且十路互异");

    // J15 渲染面：两族同名挂点渲染必不同
    let j15 = {
        let f = Hook { fam: Family::Futex, name: "渲染".to_string(), val: 4, signed: true };
        let p = Hook { fam: Family::Epoll, name: "渲染".to_string(), val: 4, signed: true };
        f.render() != p.render()
    };
    s.add("J15-渲染分旗", j15, "同值不同族渲染可区分");

    // J16 动态明细：冻结根十六进制落账
    let fp = hall.fold();
    s.add_dyn("J16-冻结根明细", fp == root, &format!("root={:#x}", fp));

    // J17 动态明细：总账缺口数双零与在册数
    let g = hall.gaps().len();
    s.add_dyn("J17-缺口明细", g == 0 && hall.hooks.len() == 20, &format!("gaps={} n={}", g, hall.hooks.len()));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11001_hall_flow() {
        let (mut l, built) = build_hall();
        assert!(built);
        // 重号面：同族同名才叫重号。初版拿全新名 "dup" 当重号样本，
        // 它根本没在册（同族满容时还先撞 CH_SHORT），自然到不了 CH_DUP。
        assert!(l.enroll(Family::Futex, &hall_name(1), 1).err() == Some(CH_DUP));
        assert!(l.enroll(Family::Futex, "zero", 0).err() == Some(CH_VALUE));
        l.freeze();
        assert!(l.enroll(Family::Epoll, "late", 1).err() == Some(CH_FROZEN));
        assert!(l.root.is_some());
    }

    #[test]
    fn unx11001_checks_green() {
        let s = run_unx11001_checks();
        assert!(s.all_passed(), "UNX-F11001 判据未全绿");
    }
}
