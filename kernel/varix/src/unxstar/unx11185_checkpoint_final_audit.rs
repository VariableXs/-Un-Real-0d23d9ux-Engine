//! UNX-F11185 域判据终盘点（800 判据唯一连续终验）（0x1f6a0 段）
//!
//! 纯功能模型：域判据以号段结算——登记须逐号连续（后号恒为前号 +1），
//! 全库 800 号须唯一且连续。重号、跳号、逆序、越界、结算后再登记一律拒止；
//! 终盘要求「唯一」与「连续」双断言同时成立，且首末号与号段钉值对平。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f6a0;
/// 号段越界
pub const CA_ERANGE: u32 = SEG_BASE + 0x01;
/// 重号
pub const CA_EDUP: u32 = SEG_BASE + 0x02;
/// 跳号（不连续）
pub const CA_EGAP: u32 = SEG_BASE + 0x03;
/// 逆序登记
pub const CA_EBACK: u32 = SEG_BASE + 0x04;
/// 结算后登记
pub const CA_EFROZEN: u32 = SEG_BASE + 0x05;
/// 结算时未满额
pub const CA_ESHORT: u32 = SEG_BASE + 0x06;
/// 首末号不对平
pub const CA_EEDGE: u32 = SEG_BASE + 0x07;

/// 终盘号数
pub const TOTAL: u32 = 800;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 终盘点：号段登记与双断言。
#[derive(Clone, Debug)]
pub struct FinalAudit {
    base: u32,
    codes: Vec<u32>,
    seen: Vec<bool>,
    settled: Option<u64>,
    pub accepts: u64,
    pub rejects: u64,
}

impl FinalAudit {
    /// 建盘：base 为号段起点，容量恰为终盘号数。
    pub fn new(base: u32) -> Self {
        FinalAudit {
            base,
            codes: Vec::with_capacity(TOTAL as usize),
            seen: vec![false; TOTAL as usize],
            settled: None,
            accepts: 0,
            rejects: 0,
        }
    }

    /// 号段上界（含）。
    pub fn edge(&self) -> u32 {
        self.base.wrapping_add(TOTAL - 1)
    }

    /// 已登记号数。
    pub fn count(&self) -> u64 {
        self.codes.len() as u64
    }

    fn slot(&self, code: u32) -> Result<usize, u32> {
        if code < self.base || code > self.edge() {
            return Err(CA_ERANGE);
        }
        Ok((code - self.base) as usize)
    }

    fn fail(&mut self, e: u32) -> Result<u64, u32> {
        self.rejects = self.rejects.wrapping_add(1);
        Err(e)
    }

    /// 登记一号：须连续（首号取号段起点），重号与跳号皆拒。
    pub fn register(&mut self, code: u32) -> Result<u64, u32> {
        if self.settled.is_some() {
            return self.fail(CA_EFROZEN);
        }
        let ix = match self.slot(code) {
            Ok(ix) => ix,
            Err(e) => return self.fail(e),
        };
        if match self.seen.get(ix) {
            Some(true) => true,
            _ => false,
        } {
            return self.fail(CA_EDUP);
        }
        let want = match self.codes.last() {
            Some(last) => last.wrapping_add(1),
            None => self.base,
        };
        if code != want {
            if code > want {
                return self.fail(CA_EGAP);
            }
            return self.fail(CA_EBACK);
        }
        if let Some(cell) = self.seen.get_mut(ix) {
            *cell = true;
        }
        self.codes.push(code);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.accepts)
    }

    /// 唯一性：登记号无重复（以位图计数自证）。
    pub fn unique(&self) -> bool {
        let mut t = 0u32;
        let mut i = 0;
        while i < self.seen.len() {
            if self.seen[i] {
                t = t.wrapping_add(1);
            }
            i += 1;
        }
        t as u64 == self.count()
    }

    /// 连续性：已登记序列逐号 +1。
    pub fn continuous(&self) -> bool {
        if self.codes.is_empty() {
            return false;
        }
        match self.codes.first() {
            Some(first) if *first == self.base => {}
            _ => return false,
        }
        let mut i = 1;
        while i < self.codes.len() {
            if self.codes[i] != self.codes[i - 1].wrapping_add(1) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 首末号对平号段钉值。
    pub fn edges_aligned(&self) -> bool {
        match (self.codes.first(), self.codes.last()) {
            (Some(f), Some(l)) => *f == self.base && *l == self.edge(),
            _ => false,
        }
    }
}

impl FinalAudit {
    /// 结算：满额 + 唯一 + 连续 + 首末对平，四者齐备方落定终盘值。
    pub fn settle(&mut self) -> Result<u64, u32> {
        if self.settled.is_some() {
            return self.fail(CA_EFROZEN);
        }
        if self.count() != TOTAL as u64 {
            return self.fail(CA_ESHORT);
        }
        if !self.unique() {
            return self.fail(CA_EDUP);
        }
        if !self.continuous() {
            return self.fail(CA_EGAP);
        }
        if !self.edges_aligned() {
            return self.fail(CA_EEDGE);
        }
        let v = self.fold();
        self.settled = Some(v);
        Ok(v)
    }

    /// 终盘指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = (h ^ self.base as u64).wrapping_mul(FNV_PRIME);
        let mut i = 0;
        while i < self.codes.len() {
            h = (h ^ self.codes[i] as u64).wrapping_mul(FNV_PRIME);
            i += 1;
        }
        h = (h ^ self.accepts).wrapping_mul(FNV_PRIME);
        h
    }

    /// 冻结（结算即冻结；未结算取当下指纹作占位）。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.settled.is_none() {
            self.settled = Some(self.fold());
        }
        self.settled
    }

    pub fn is_settled(&self) -> bool {
        self.settled.is_some()
    }

    /// 未登记号数（缺口读数，终盘须为零）。
    pub fn holes(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0;
        while i < self.seen.len() {
            if !self.seen[i] {
                n = n.wrapping_add(1);
            }
            i += 1;
        }
        n
    }
}

/// 标准终盘：自 base 起逐号登记满 800 号并结算。
pub fn build_audit(base: u32) -> (FinalAudit, bool) {
    let mut a = FinalAudit::new(base);
    let mut n = 0u32;
    while n < TOTAL {
        if a.register(base.wrapping_add(n)).is_err() {
            return (a, false);
        }
        n = n.wrapping_add(1);
    }
    let ok = a.settle().is_ok();
    (a, ok)
}

/// 四类注入：越界号 / 重号 / 跳号 / 结算后登记，全部须拒止。
pub fn inject_four() -> [bool; 4] {
    let base = 0x1000u32;

    // ① 越界号
    let mut a0 = FinalAudit::new(base);
    let a = a0.register(base - 1).err() == Some(CA_ERANGE)
        && a0.register(base + TOTAL).err() == Some(CA_ERANGE);

    // ② 重号：先连续登记两号，再回填首号
    let mut a1 = FinalAudit::new(base);
    let r0 = a1.register(base).is_ok();
    let r1 = a1.register(base + 1).is_ok();
    let b = r0 && r1 && a1.register(base).err() == Some(CA_EDUP);

    // ③ 跳号
    let mut a2 = FinalAudit::new(base);
    let _ = a2.register(base);
    let c = a2.register(base + 2).err() == Some(CA_EGAP);

    // ④ 结算后登记与未满结算
    let (mut full, built) = build_audit(base);
    let short_ok = {
        let mut s = FinalAudit::new(base);
        let _ = s.register(base);
        s.settle().err() == Some(CA_ESHORT)
    };
    let d = built && short_ok && full.register(base + 999).err() == Some(CA_EFROZEN);

    [a, b, c, d]
}

/// 判据台：域判据终盘点的十三条判据。
pub fn run_unx11185_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11185");

    let (mut a, built) = build_audit(0x1000);
    let root = a.freeze();

    // J1 全库终盘逐号对平
    s.add(
        "J1-逐号对平",
        built && a.count() == TOTAL as u64 && a.holes() == 0 && a.is_settled(),
        "800 号逐号登记齐备且结算落定",
    );

    // J2 唯一连续双断言
    s.add(
        "J2-唯一连续",
        a.unique() && a.continuous(),
        "无重号且逐号 +1 连续",
    );

    // J3 首末对平
    s.add(
        "J3-首末对平",
        a.edges_aligned() && a.edge() == 0x1000 + TOTAL - 1,
        "首号等于号段起点、末号等于号段上界",
    );

    // J4 结算值等于指纹
    s.add(
        "J4-结算指纹",
        root == Some(a.fold()) && a.freeze() == root,
        "结算值即指纹且冻结幂等",
    );

    // J5 结算后拒登记
    s.add(
        "J5-结算拒登记",
        a.register(0x1000).err() == Some(CA_EFROZEN)
            && a.settle() == Err(CA_EFROZEN),
        "结算后登记与二次结算皆拒",
    );

    // J6 重号拒止
    let mut d = FinalAudit::new(1);
    let r0 = d.register(1).is_ok();
    let r1 = d.register(2).is_ok();
    s.add(
        "J6-重号拒止",
        r0 && r1 && d.register(1).err() == Some(CA_EDUP) && d.count() == 2,
        "回填已登记号拒止且计数不变",
    );

    // J7 跳号拒止
    let mut g = FinalAudit::new(1);
    let _ = g.register(1);
    s.add(
        "J7-跳号拒止",
        g.register(3).err() == Some(CA_EGAP) && g.count() == 1,
        "越前一号登记拒止",
    );

    // J8 逆序拒止
    let mut b = FinalAudit::new(1);
    let _ = b.register(1);
    let _ = b.register(2);
    s.add(
        "J8-逆序拒止",
        b.register(1).err() == Some(CA_EDUP),
        "回退号被重号规则拒止",
    );

    // J9 号段越界拒止
    let mut o = FinalAudit::new(0x2000);
    s.add(
        "J9-号段越界",
        o.register(0x1fff).err() == Some(CA_ERANGE)
            && o.register(0x2000 + TOTAL).err() == Some(CA_ERANGE),
        "号段上下界之外双路拒止",
    );

    // J10 未满结算拒止
    let mut sh = FinalAudit::new(1);
    let mut k = 0u32;
    while k < TOTAL - 1 {
        let _ = sh.register(1 + k);
        k = k.wrapping_add(1);
    }
    s.add(
        "J10-未满拒结算",
        sh.settle().err() == Some(CA_ESHORT) && sh.count() == TOTAL as u64 - 1,
        "差一号不得结算",
    );

    // J11 注入四拒
    let hits = inject_four();
    let mut all = true;
    let mut i = 0;
    while i < hits.len() {
        if !hits[i] {
            all = false;
        }
        i += 1;
    }
    s.add("J11-注入四拒", all, "越界/重号/跳号/结算后登记四路全拒");

    // J12 重算一致
    let (x1, ok1) = build_audit(0x3000);
    let (x2, ok2) = build_audit(0x3000);
    s.add(
        "J12-重算一致",
        ok1 && ok2 && x1.fold() == x2.fold(),
        "同号段两次终盘指纹一致",
    );

    // J13 动态明细：以全新实例重算，避免 J5 的对抗写入污染拒计
    // build_audit 内部已结算，此处只取终态，不再二次结算
    let (mut f13, ok13) = build_audit(0x1000);
    let settled13 = ok13;
    let root13 = f13.freeze().unwrap_or(0);
    let (c13, h13) = (f13.count(), f13.holes());
    s.add_dyn(
        "J13-终盘明细",
        ok13 && settled13 && f13.rejects == 0 && f13.holes() == 0 && f13.unique() && f13.continuous(),
        &format!(
            "base={:#x} count={} holes={} rej={} settled={} root={:#x}",
            f13.base, c13, h13, f13.rejects, settled13, root13
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11185_audit_flow() {
        let (a, ok) = build_audit(0x1000);
        assert!(ok);
        assert!(a.is_settled());
        assert_eq!(a.count(), TOTAL as u64);
        assert_eq!(a.holes(), 0);
        assert!(a.unique());
        assert!(a.continuous());
        assert!(a.edges_aligned());
        assert!(inject_four().iter().all(|h| *h));

        let mut e = FinalAudit::new(9);
        assert_eq!(e.settle(), Err(CA_ESHORT));
    }

    #[test]
    fn unx11185_checks_green() {
        let s = run_unx11185_checks();
        assert!(s.all_passed(), "UNX-F11185 判据未全绿");
    }
}
