//! UNX-F11151 B31–B32 清账（挂号联签两批判据终重放）（0x1f330 段）
//!
//! 纯功能模型：B31/B32 两批采用「挂号—联签」制，每批判据重放后须由两名签约人
//! 联署方能挂号。清账要求两批齐、每批 20 条判据、联签恰 2 人、争议与处置数
//! 对平；任一条件不成立即拒清，终值冻结后不再收挂号件。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f330;
/// 批号越界（只认 B31/B32）
pub const SG_ENO: u32 = SEG_BASE + 0x01;
/// 批重复挂号
pub const SG_EDUP: u32 = SEG_BASE + 0x02;
/// 挂号批序倒挂
pub const SG_EORDER: u32 = SEG_BASE + 0x03;
/// 联签人数不为 2 或判据重放为零（形状违规）
pub const SG_ESHAPE: u32 = SEG_BASE + 0x04;
/// 两批未齐即清账
pub const SG_ESHORT: u32 = SEG_BASE + 0x05;
/// 判据条数不足（批内非 20、合计非 40）
pub const SG_ESUM: u32 = SEG_BASE + 0x06;
/// 争议未处置干净
pub const SG_EUNRES: u32 = SEG_BASE + 0x07;
/// 终值已锁定
pub const SG_EFROZEN: u32 = SEG_BASE + 0x08;

/// 本段批数（B31、B32）。
pub const BATCHES: usize = 2;
/// 首批判号。
pub const FIRST_BATCH: u32 = 31;
/// 每批判据条数。
pub const CRITERIA_PER_BATCH: u16 = 20;
/// 联签人数口径。
pub const SIGNERS_REQUIRED: u8 = 2;
/// 两批判据合计。
pub const SEG_CRITERIA: u32 = 40;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一批挂号联签件。
#[derive(Clone, Copy, Debug)]
pub struct Signoff {
    pub batch: u8,
    pub replayed: u16,
    pub signers: u8,
    pub disputes: u32,
    pub resolved: u32,
}

impl Signoff {
    /// 联签是否闭合（两人签、判据足、争议处置完）。
    pub fn cleared(&self) -> bool {
        self.signers == SIGNERS_REQUIRED
            && self.replayed == CRITERIA_PER_BATCH
            && self.disputes == self.resolved
    }
}

/// B31/B32 挂号联签账。
#[derive(Clone, Debug)]
pub struct SignoffLedger {
    items: Vec<Signoff>,
    settled: Option<u64>,
    frozen: bool,
    pub accepts: u64,
    pub rejects: u64,
}

impl SignoffLedger {
    /// 空账。
    pub fn new() -> SignoffLedger {
        SignoffLedger {
            items: Vec::new(),
            settled: None,
            frozen: false,
            accepts: 0,
            rejects: 0,
        }
    }

    /// 已挂号批数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 取某批挂号件。
    pub fn item(&self, batch: u32) -> Option<Signoff> {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].batch as u32 == batch {
                return Some(self.items[i]);
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 挂一批：冻结 → 越界 → 重号 → 倒序 → 形状 → 争议越界。
    ///
    /// 重号先于倒序：同一批再挂一次只报 SG_EDUP，不会误报 SG_EORDER。
    pub fn register(
        &mut self,
        batch: u32,
        replayed: u16,
        signers: u8,
        disputes: u32,
        resolved: u32,
    ) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(SG_EFROZEN);
        }
        if batch < FIRST_BATCH || batch >= FIRST_BATCH.wrapping_add(BATCHES as u32) {
            return self.fail(SG_ENO);
        }
        if self.item(batch).is_some() {
            return self.fail(SG_EDUP);
        }
        if let Some(last) = self.items.last() {
            if batch <= last.batch as u32 {
                return self.fail(SG_EORDER);
            }
        }
        if signers != SIGNERS_REQUIRED || replayed == 0 {
            return self.fail(SG_ESHAPE);
        }
        if resolved > disputes {
            return self.fail(SG_ESHAPE);
        }
        self.items.push(Signoff {
            batch: batch as u8,
            replayed,
            signers,
            disputes,
            resolved,
        });
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> Result<u64, u32> {
        self.rejects = self.rejects.wrapping_add(1);
        Err(code)
    }
}

impl SignoffLedger {
    /// 判据合计。
    pub fn criteria_sum(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            t = t.wrapping_add(self.items[i].replayed as u32);
            i = i.wrapping_add(1);
        }
        t
    }

    /// 联签人次合计。
    pub fn signer_sum(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            t = t.wrapping_add(self.items[i].signers as u32);
            i = i.wrapping_add(1);
        }
        t
    }

    /// 争议与处置合计。
    pub fn dispute_totals(&self) -> (u32, u32) {
        let mut d = 0u32;
        let mut r = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            d = d.wrapping_add(self.items[i].disputes);
            r = r.wrapping_add(self.items[i].resolved);
            i = i.wrapping_add(1);
        }
        (d, r)
    }

    /// 缺挂号批表。
    pub fn gaps(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut b = FIRST_BATCH;
        let end = FIRST_BATCH.wrapping_add(BATCHES as u32);
        while b < end {
            if self.item(b).is_none() {
                out.push(b);
            }
            b = b.wrapping_add(1);
        }
        out
    }

    /// 批序严格递增。
    pub fn order_strict(&self) -> bool {
        let mut i = 1usize;
        while i < self.items.len() {
            if self.items[i].batch <= self.items[i - 1].batch {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 联签是否全闭合。
    pub fn all_cleared(&self) -> bool {
        let mut i = 0usize;
        while i < self.items.len() {
            if !self.items[i].cleared() {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 挂号联签指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.items.len() {
            let it = self.items[i];
            h = (h ^ it.batch as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ it.replayed as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ it.signers as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ it.disputes as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ it.resolved as u64).wrapping_mul(FNV_PRIME);
            i = i.wrapping_add(1);
        }
        h = (h ^ self.items.len() as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.criteria_sum() as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.signer_sum() as u64).wrapping_mul(FNV_PRIME);
        h
    }

    /// 终清账：两批齐 → 判据合计 → 争议清零 → 逐批闭合。
    pub fn settle(&mut self) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(SG_EFROZEN);
        }
        if self.items.len() != BATCHES || !self.gaps().is_empty() {
            return self.fail(SG_ESHORT);
        }
        if self.criteria_sum() != SEG_CRITERIA {
            return self.fail(SG_ESUM);
        }
        let (d, r) = self.dispute_totals();
        if d != r {
            return self.fail(SG_EUNRES);
        }
        if !self.all_cleared() {
            return self.fail(SG_EUNRES);
        }
        let v = self.fold();
        self.settled = Some(v);
        Ok(v)
    }

    /// 冻结：锁当前值，幂等。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.settled;
        }
        if self.settled.is_none() {
            self.settled = Some(self.fold());
        }
        self.frozen = true;
        self.settled
    }

    /// 终值。
    pub fn settled_value(&self) -> Option<u64> {
        self.settled
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 建账：B31/B32 各 20 判据、各 2 人联签、零争议，再清账。
pub fn build_b3132() -> (SignoffLedger, bool) {
    let mut l = SignoffLedger::new();
    let mut i = 0usize;
    while i < BATCHES {
        let batch = FIRST_BATCH.wrapping_add(i as u32);
        if l.register(batch, CRITERIA_PER_BATCH, SIGNERS_REQUIRED, 0, 0)
            .is_err()
        {
            return (l, false);
        }
        i = i.wrapping_add(1);
    }
    let ok = l.settle().is_ok();
    (l, ok)
}

/// 四类注入：批号越界 / 重号与倒序 / 联签形状与争议越界 / 终账后再挂。
pub fn inject_four() -> [bool; 4] {
    // ① 批号越界
    let mut a0 = SignoffLedger::new();
    let r0 = a0
        .register(FIRST_BATCH.wrapping_sub(1), 20, 2, 0, 0)
        .err()
        == Some(SG_ENO)
        && a0
            .register(FIRST_BATCH.wrapping_add(BATCHES as u32), 20, 2, 0, 0)
            .err()
            == Some(SG_ENO)
        && a0.is_empty();

    // ② 重号与倒序
    let mut a1 = SignoffLedger::new();
    let s0 = a1.register(31, 20, 2, 0, 0).is_ok();
    let s1 = a1.register(32, 20, 2, 0, 0).is_ok();
    let dup = s0 && s1 && a1.register(31, 20, 2, 0, 0).err() == Some(SG_EDUP);
    let mut a2 = SignoffLedger::new();
    let _ = a2.register(32, 20, 2, 0, 0);
    let order = a2.register(31, 20, 2, 0, 0).err() == Some(SG_EORDER);
    let r1 = dup && order;

    // ③ 联签形状与争议越界
    let mut a3 = SignoffLedger::new();
    let shape = a3.register(31, 20, 1, 0, 0).err() == Some(SG_ESHAPE)
        && a3.register(31, 20, 2, 3, 4).err() == Some(SG_ESHAPE)
        && a3.register(31, 0, 2, 0, 0).err() == Some(SG_ESHAPE);
    let mut a4 = SignoffLedger::new();
    let _ = a4.register(31, 20, 2, 2, 1);
    let _ = a4.register(32, 20, 2, 0, 0);
    let unres = a4.settle().err() == Some(SG_EUNRES);
    let r2 = shape && unres;

    // ④ 终账后再挂 / 再清账
    let (mut a5, ok5) = build_b3132();
    let locked = a5.freeze().is_some();
    let r3 = ok5
        && locked
        && a5.register(31, 20, 2, 0, 0).err() == Some(SG_EFROZEN)
        && a5.settle().err() == Some(SG_EFROZEN);

    [r0, r1, r2, r3]
}

/// 判据：UNX-F11151 B31–B32 挂号联签两批判据终重放。
pub fn run_unx11151_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11151");
    let (mut l, ok) = build_b3132();

    // X1 两批齐备
    s.add_dyn(
        "X1-两批齐备",
        ok && l.len() == BATCHES && l.gaps().is_empty(),
        &format!("batch={}/{} gaps={}", l.len(), BATCHES, l.gaps().len()),
    );

    // X2 判据合计对平
    s.add_dyn(
        "X2-判据合计对平",
        l.criteria_sum() == SEG_CRITERIA,
        &format!("criteria={}/{}", l.criteria_sum(), SEG_CRITERIA),
    );

    // X3 联签人次对平
    let want_signers = (BATCHES as u32).wrapping_mul(SIGNERS_REQUIRED as u32);
    s.add_dyn(
        "X3-联签人次对平",
        l.signer_sum() == want_signers,
        &format!("signers={}/{}", l.signer_sum(), want_signers),
    );

    // X4 逐批闭合（20 条 / 2 签）
    s.add(
        "X4-逐批闭合",
        l.all_cleared()
            && l.item(31).map(|x| x.replayed) == Some(CRITERIA_PER_BATCH)
            && l.item(32).map(|x| x.signers) == Some(SIGNERS_REQUIRED),
        "B31/B32 各 20 条判据、各 2 人联签",
    );

    // X5 争议处置对平
    let (d, r) = l.dispute_totals();
    s.add_dyn(
        "X5-争议处置对平",
        d == r && d == 0,
        &format!("disputes={} resolved={}", d, r),
    );

    // X6 批序严格
    s.add("X6-批序严格", l.order_strict(), "挂号批号 B31 → B32 严格递增");

    // X7 终值即指纹
    let root = l.settled_value();
    s.add(
        "X7-终值即指纹",
        root == Some(l.fold()),
        "清账单值等于挂号联签件指纹",
    );

    // X8 冻结幂等
    let f1 = l.freeze();
    let f2 = l.freeze();
    s.add("X8-冻结幂等", f1.is_some() && f1 == f2, "二次冻结返回同一单值");

    // X9 冻结后拒挂拒清
    s.add(
        "X9-冻结后拒挂拒清",
        l.is_frozen()
            && l.register(31, 20, 2, 0, 0).err() == Some(SG_EFROZEN)
            && l.settle().err() == Some(SG_EFROZEN),
        "冻结后 register/settle 都落 SG_EFROZEN",
    );

    // X10 批号越界拒止
    let mut g = SignoffLedger::new();
    s.add(
        "X10-批号越界拒止",
        g.register(30, 20, 2, 0, 0).err() == Some(SG_ENO)
            && g.register(33, 20, 2, 0, 0).err() == Some(SG_ENO)
            && g.is_empty(),
        "B31/B32 之外的批号一律 SG_ENO",
    );

    // X11 重号先于倒序
    let mut d2 = SignoffLedger::new();
    let _ = d2.register(31, 20, 2, 0, 0);
    let _ = d2.register(32, 20, 2, 0, 0);
    let mut o2 = SignoffLedger::new();
    let _ = o2.register(32, 20, 2, 0, 0);
    s.add(
        "X11-重号先于倒序",
        d2.register(31, 20, 2, 0, 0).err() == Some(SG_EDUP)
            && o2.register(31, 20, 2, 0, 0).err() == Some(SG_EORDER),
        "同批重挂报 EDUP，真倒序才报 EORDER",
    );

    // X12 形状与争议越界拒止
    let mut sh = SignoffLedger::new();
    s.add(
        "X12-形状与争议拒止",
        sh.register(31, 20, 1, 0, 0).err() == Some(SG_ESHAPE)
            && sh.register(31, 20, 2, 3, 4).err() == Some(SG_ESHAPE)
            && sh.register(31, 0, 2, 0, 0).err() == Some(SG_ESHAPE)
            && sh.is_empty(),
        "单人签、处置超争议、零判据都落 SG_ESHAPE",
    );

    // X13 清账三拒与注入四拒
    let mut q1 = SignoffLedger::new();
    let _ = q1.register(31, 20, 2, 0, 0);
    let mut q2 = SignoffLedger::new();
    let _ = q2.register(31, 21, 2, 0, 0);
    let _ = q2.register(32, 20, 2, 0, 0);
    let mut q3 = SignoffLedger::new();
    let _ = q3.register(31, 20, 2, 2, 1);
    let _ = q3.register(32, 20, 2, 0, 0);
    let hits = inject_four();
    let all = hits[0] && hits[1] && hits[2] && hits[3];
    let (mut fresh, ok2) = build_b3132();
    let root2 = fresh.freeze().unwrap_or(0);
    s.add_dyn(
        "X13-清账三拒与注入",
        q1.settle().err() == Some(SG_ESHORT)
            && q2.settle().err() == Some(SG_ESUM)
            && q3.settle().err() == Some(SG_EUNRES)
            && all
            && ok2
            && fresh.accepts == BATCHES as u64
            && fresh.rejects == 0,
        &format!(
            "short/sum/unres ok inject={}{}{}{} acc={} rej={} root={:#x}",
            hits[0] as u8, hits[1] as u8, hits[2] as u8, hits[3] as u8,
            fresh.accepts, fresh.rejects, root2
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11151_flow() {
        let (mut l, ok) = build_b3132();
        assert!(ok);
        assert_eq!(l.len(), BATCHES);
        assert!(l.gaps().is_empty());
        assert!(l.order_strict());
        assert!(l.all_cleared());
        assert_eq!(l.criteria_sum(), SEG_CRITERIA);
        assert_eq!(l.signer_sum(), 4);
        let root = l.settled_value();
        assert_eq!(root, Some(l.fold()));
        assert_eq!(l.freeze(), root);
        assert_eq!(l.freeze(), root);
        assert!(l.is_frozen());
        assert_eq!(l.register(31, 20, 2, 0, 0), Err(SG_EFROZEN));
        assert_eq!(l.settle(), Err(SG_EFROZEN));
    }

    #[test]
    fn f11151_gates() {
        let mut l = SignoffLedger::new();
        assert!(l.register(31, 20, 2, 0, 0).is_ok());
        assert!(l.register(32, 20, 2, 0, 0).is_ok());
        assert_eq!((l.accepts, l.rejects), (2, 0));
        assert_eq!(l.register(30, 20, 2, 0, 0), Err(SG_ENO));
        assert_eq!(l.register(31, 20, 2, 0, 0), Err(SG_EDUP));
        assert_eq!(l.register(32, 20, 2, 0, 0), Err(SG_EDUP));
        assert_eq!((l.accepts, l.rejects), (2, 3));

        let mut sh = SignoffLedger::new();
        assert_eq!(sh.register(31, 20, 1, 0, 0), Err(SG_ESHAPE));
        assert_eq!(sh.register(31, 20, 2, 3, 4), Err(SG_ESHAPE));
        assert_eq!((sh.accepts, sh.rejects), (0, 2));

        let mut o = SignoffLedger::new();
        let _ = o.register(32, 20, 2, 0, 0);
        assert_eq!(o.register(31, 20, 2, 0, 0), Err(SG_EORDER));
        assert_eq!((o.accepts, o.rejects), (1, 1));

        let mut q = SignoffLedger::new();
        let _ = q.register(31, 20, 2, 0, 0);
        assert_eq!(q.settle(), Err(SG_ESHORT));
        assert_eq!((q.accepts, q.rejects), (1, 1));

        let mut m = SignoffLedger::new();
        let _ = m.register(31, 21, 2, 0, 0);
        let _ = m.register(32, 20, 2, 0, 0);
        assert_eq!(m.settle(), Err(SG_ESUM));
        assert_eq!((m.accepts, m.rejects), (2, 1));

        let mut u = SignoffLedger::new();
        let _ = u.register(31, 20, 2, 3, 1);
        let _ = u.register(32, 20, 2, 0, 0);
        assert_eq!(u.settle(), Err(SG_EUNRES));
        assert_eq!((u.accepts, u.rejects), (2, 1));

        let (mut d, ok) = build_b3132();
        assert!(ok);
        assert!(d.freeze().is_some());
        assert_eq!(d.register(31, 20, 2, 0, 0), Err(SG_EFROZEN));
        assert_eq!(d.settle(), Err(SG_EFROZEN));
        assert_eq!((d.accepts, d.rejects), (2, 2));
    }

    #[test]
    fn f11151_checks_green() {
        assert!(run_unx11151_checks().all_passed());
    }
}
