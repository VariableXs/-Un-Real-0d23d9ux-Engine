//! UNX-F11154 B35–B36 清账（演习与 I 型收口两批判据终重放）（0x1f360 段）
//!
//! 纯功能模型：B35 为演习批、B36 为 I 型收口批，两批各 20 条判据。终重放要求
//! 批种与批号严格对应（35 演习、36 收口），两批发现项全部处置闭合，判据合计
//! 40；两批终值一并冻结后，B35/B36 不再收件。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f360;
/// 批号越界（只认 B35/B36）
pub const BX_ENO: u32 = SEG_BASE + 0x01;
/// 批重复落账
pub const BX_EDUP: u32 = SEG_BASE + 0x02;
/// 批序倒挂
pub const BX_EORDER: u32 = SEG_BASE + 0x03;
/// 批种与批号不符（形状违规）
pub const BX_EKIND: u32 = SEG_BASE + 0x04;
/// 两批未齐即清账
pub const BX_ESHORT: u32 = SEG_BASE + 0x05;
/// 判据合计不足 40
pub const BX_ESUM: u32 = SEG_BASE + 0x06;
/// 发现项未处置闭合
pub const BX_EOPEN: u32 = SEG_BASE + 0x07;
/// 终值已锁定
pub const BX_EFROZEN: u32 = SEG_BASE + 0x08;

/// 本段批数。
pub const BATCHES: usize = 2;
/// 首批判号。
pub const FIRST_BATCH: u32 = 35;
/// 批种：演习。
pub const KIND_DRILL: u8 = 0;
/// 批种：I 型收口。
pub const KIND_ITYPE: u8 = 1;
/// 批种域宽度。
pub const KINDS: u8 = 2;
/// 每批判据条数。
pub const CRITERIA_PER_BATCH: u16 = 20;
/// 段判据合计。
pub const SEG_CRITERIA: u32 = 40;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一批演习/收口件。
#[derive(Clone, Copy, Debug)]
pub struct BatchEntry {
    pub batch: u8,
    pub kind: u8,
    pub criteria: u16,
    pub findings: u32,
    pub closed: u32,
}

impl BatchEntry {
    /// 批种是否与批号口径一致。
    pub fn kind_matches(&self) -> bool {
        let want = if self.batch as u32 == FIRST_BATCH {
            KIND_DRILL
        } else {
            KIND_ITYPE
        };
        self.kind == want
    }

    /// 发现项是否全处置。
    pub fn closed_out(&self) -> bool {
        self.findings == self.closed && self.criteria == CRITERIA_PER_BATCH
    }
}

/// B35/B36 演习与 I 型收口账。
#[derive(Clone, Debug)]
pub struct DrillLedger {
    items: Vec<BatchEntry>,
    settled: Option<u64>,
    frozen: bool,
    pub accepts: u64,
    pub rejects: u64,
}

impl DrillLedger {
    /// 空账。
    pub fn new() -> DrillLedger {
        DrillLedger {
            items: Vec::new(),
            settled: None,
            frozen: false,
            accepts: 0,
            rejects: 0,
        }
    }

    /// 已落批数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 取某批。
    pub fn item(&self, batch: u32) -> Option<BatchEntry> {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].batch as u32 == batch {
                return Some(self.items[i]);
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 落一批：冻结 → 越界 → 重记 → 倒序 → 批种 → 处置越界。
    ///
    /// 重记先于倒序，批种先于处置越界：同一批重落只报 BX_EDUP。
    pub fn record(
        &mut self,
        batch: u32,
        kind: u8,
        criteria: u16,
        findings: u32,
        closed: u32,
    ) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(BX_EFROZEN);
        }
        if batch < FIRST_BATCH || batch >= FIRST_BATCH.wrapping_add(BATCHES as u32) {
            return self.fail(BX_ENO);
        }
        if self.item(batch).is_some() {
            return self.fail(BX_EDUP);
        }
        if let Some(last) = self.items.last() {
            if batch <= last.batch as u32 {
                return self.fail(BX_EORDER);
            }
        }
        if kind >= KINDS {
            return self.fail(BX_EKIND);
        }
        let entry = BatchEntry {
            batch: batch as u8,
            kind,
            criteria,
            findings,
            closed,
        };
        if !entry.kind_matches() {
            return self.fail(BX_EKIND);
        }
        if closed > findings {
            return self.fail(BX_EKIND);
        }
        self.items.push(entry);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> Result<u64, u32> {
        self.rejects = self.rejects.wrapping_add(1);
        Err(code)
    }

    /// 判据合计。
    pub fn criteria_sum(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            t = t.wrapping_add(self.items[i].criteria as u32);
            i = i.wrapping_add(1);
        }
        t
    }

    /// 发现项与处置合计。
    pub fn finding_totals(&self) -> (u32, u32) {
        let mut f = 0u32;
        let mut c = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            f = f.wrapping_add(self.items[i].findings);
            c = c.wrapping_add(self.items[i].closed);
            i = i.wrapping_add(1);
        }
        (f, c)
    }

    /// 缺批表。
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

    /// 批序严格。
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

    /// 两批批种都对号。
    pub fn kinds_aligned(&self) -> bool {
        let mut i = 0usize;
        while i < self.items.len() {
            if !self.items[i].kind_matches() {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 演习与收口指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.items.len() {
            let b = self.items[i];
            h = (h ^ b.batch as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ b.kind as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ b.criteria as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ b.findings as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ b.closed as u64).wrapping_mul(FNV_PRIME);
            i = i.wrapping_add(1);
        }
        h = (h ^ self.items.len() as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.criteria_sum() as u64).wrapping_mul(FNV_PRIME);
        h
    }

    /// 终清账：两批齐 → 判据合计 → 批种对号 → 处置闭合。
    pub fn settle(&mut self) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(BX_EFROZEN);
        }
        if self.items.len() != BATCHES || !self.gaps().is_empty() {
            return self.fail(BX_ESHORT);
        }
        if self.criteria_sum() != SEG_CRITERIA {
            return self.fail(BX_ESUM);
        }
        if !self.kinds_aligned() {
            return self.fail(BX_EKIND);
        }
        let (f, c) = self.finding_totals();
        if f != c {
            return self.fail(BX_EOPEN);
        }
        let mut i = 0usize;
        while i < self.items.len() {
            if !self.items[i].closed_out() {
                return self.fail(BX_EOPEN);
            }
            i = i.wrapping_add(1);
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

/// 建账：B35 演习、B36 收口，各 20 判据、零发现项，再清账。
pub fn build_b3536() -> (DrillLedger, bool) {
    let mut l = DrillLedger::new();
    if l.record(35, KIND_DRILL, CRITERIA_PER_BATCH, 0, 0).is_err() {
        return (l, false);
    }
    if l.record(36, KIND_ITYPE, CRITERIA_PER_BATCH, 0, 0).is_err() {
        return (l, false);
    }
    let ok = l.settle().is_ok();
    (l, ok)
}

/// 四类注入：批号越界 / 重记与倒序 / 批种错配与处置越界 / 终账后再落。
pub fn inject_four() -> [bool; 4] {
    // ① 批号越界
    let mut a0 = DrillLedger::new();
    let r0 = a0
        .record(34, KIND_DRILL, 20, 0, 0)
        .err()
        == Some(BX_ENO)
        && a0.record(37, KIND_ITYPE, 20, 0, 0).err() == Some(BX_ENO)
        && a0.is_empty();

    // ② 重记与倒序
    let mut a1 = DrillLedger::new();
    let s0 = a1.record(35, KIND_DRILL, 20, 0, 0).is_ok();
    let s1 = a1.record(36, KIND_ITYPE, 20, 0, 0).is_ok();
    let dup = s0 && s1 && a1.record(35, KIND_DRILL, 20, 0, 0).err() == Some(BX_EDUP);
    let mut a2 = DrillLedger::new();
    let _ = a2.record(36, KIND_ITYPE, 20, 0, 0);
    let order = a2.record(35, KIND_DRILL, 20, 0, 0).err() == Some(BX_EORDER);
    let r1 = dup && order;

    // ③ 批种错配与处置越界
    let mut a3 = DrillLedger::new();
    let kind = a3.record(35, KIND_ITYPE, 20, 0, 0).err() == Some(BX_EKIND)
        && a3.record(35, KIND_DRILL, 20, 1, 3).err() == Some(BX_EKIND);
    let mut a4 = DrillLedger::new();
    let _ = a4.record(35, KIND_DRILL, 20, 2, 1);
    let _ = a4.record(36, KIND_ITYPE, 20, 0, 0);
    let open = a4.settle().err() == Some(BX_EOPEN);
    let r2 = kind && open;

    // ④ 终账后再落 / 再清账
    let (mut a5, ok5) = build_b3536();
    let locked = a5.freeze().is_some();
    let r3 = ok5
        && locked
        && a5.record(35, KIND_DRILL, 20, 0, 0).err() == Some(BX_EFROZEN)
        && a5.settle().err() == Some(BX_EFROZEN);

    [r0, r1, r2, r3]
}

/// 判据：UNX-F11154 B35–B36 演习与 I 型收口两批判据终重放。
pub fn run_unx11154_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11154");
    let (mut l, ok) = build_b3536();

    // X1 两批齐备
    s.add_dyn(
        "X1-两批齐备",
        ok && l.len() == BATCHES && l.gaps().is_empty(),
        &format!("batch={}/{} gaps={}", l.len(), BATCHES, l.gaps().len()),
    );

    // X2 判据合计
    s.add_dyn(
        "X2-判据合计对平",
        l.criteria_sum() == SEG_CRITERIA,
        &format!("criteria={}/{}", l.criteria_sum(), SEG_CRITERIA),
    );

    // X3 批种对号
    s.add(
        "X3-批种对号",
        l.kinds_aligned()
            && l.item(35).map(|x| x.kind) == Some(KIND_DRILL)
            && l.item(36).map(|x| x.kind) == Some(KIND_ITYPE),
        "B35 演习、B36 I 型收口，批种与批号一致",
    );

    // X4 发现项处置闭合
    let (f, c) = l.finding_totals();
    s.add_dyn(
        "X4-处置闭合",
        f == c && f == 0,
        &format!("findings={} closed={}", f, c),
    );

    // X5 批序严格
    s.add("X5-批序严格", l.order_strict(), "B35 → B36 严格递增");

    // X6 终值即指纹
    let root = l.settled_value();
    s.add("X6-终值即指纹", root == Some(l.fold()), "两批清单值等于逐批指纹");

    // X7 冻结幂等
    let f1 = l.freeze();
    let f2 = l.freeze();
    s.add("X7-冻结幂等", f1.is_some() && f1 == f2, "二次冻结返回同一单值");

    // X8 冻结后拒记拒清
    s.add(
        "X8-冻结后拒记拒清",
        l.is_frozen()
            && l.record(35, KIND_DRILL, 20, 0, 0).err() == Some(BX_EFROZEN)
            && l.settle().err() == Some(BX_EFROZEN),
        "冻结后 record/settle 都落 BX_EFROZEN",
    );

    // X9 批号越界拒止
    let mut g = DrillLedger::new();
    s.add(
        "X9-批号越界拒止",
        g.record(34, KIND_DRILL, 20, 0, 0).err() == Some(BX_ENO)
            && g.record(37, KIND_ITYPE, 20, 0, 0).err() == Some(BX_ENO)
            && g.is_empty(),
        "B35/B36 之外的批号一律 BX_ENO",
    );

    // X10 重记先于倒序
    let mut d = DrillLedger::new();
    let _ = d.record(35, KIND_DRILL, 20, 0, 0);
    let _ = d.record(36, KIND_ITYPE, 20, 0, 0);
    let mut o = DrillLedger::new();
    let _ = o.record(36, KIND_ITYPE, 20, 0, 0);
    s.add(
        "X10-重记先于倒序",
        d.record(35, KIND_DRILL, 20, 0, 0).err() == Some(BX_EDUP)
            && o.record(35, KIND_DRILL, 20, 0, 0).err() == Some(BX_EORDER),
        "同批重落报 EDUP，真倒序才报 EORDER",
    );

    // X11 批种与处置越界拒止
    let mut sh = DrillLedger::new();
    s.add(
        "X11-批种与处置拒止",
        sh.record(35, KIND_ITYPE, 20, 0, 0).err() == Some(BX_EKIND)
            && sh.record(35, KIND_DRILL, 20, 1, 3).err() == Some(BX_EKIND)
            && sh.record(36, KIND_DRILL, 20, 0, 0).err() == Some(BX_EKIND)
            && sh.is_empty(),
        "批种错配与处置超发现都落 BX_EKIND",
    );

    // X12 短账 / 判据不足 / 发现未闭
    let mut q1 = DrillLedger::new();
    let _ = q1.record(35, KIND_DRILL, 20, 0, 0);
    let mut q2 = DrillLedger::new();
    let _ = q2.record(35, KIND_DRILL, 19, 0, 0);
    let _ = q2.record(36, KIND_ITYPE, 20, 0, 0);
    let mut q3 = DrillLedger::new();
    let _ = q3.record(35, KIND_DRILL, 20, 2, 1);
    let _ = q3.record(36, KIND_ITYPE, 20, 0, 0);
    let hits = inject_four();
    let all = hits[0] && hits[1] && hits[2] && hits[3];
    let (mut fresh, ok2) = build_b3536();
    let root2 = fresh.freeze().unwrap_or(0);
    s.add_dyn(
        "X12-三拒与注入四拒",
        q1.settle().err() == Some(BX_ESHORT)
            && q2.settle().err() == Some(BX_ESUM)
            && q3.settle().err() == Some(BX_EOPEN)
            && all
            && ok2
            && fresh.accepts == BATCHES as u64
            && fresh.rejects == 0
            && root2 != 0,
        &format!(
            "inject={}{}{}{} acc={} rej={} root={:#x}",
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
    fn f11154_flow() {
        let (mut l, ok) = build_b3536();
        assert!(ok);
        assert_eq!(l.len(), BATCHES);
        assert!(l.gaps().is_empty());
        assert!(l.kinds_aligned());
        assert!(l.order_strict());
        assert_eq!(l.criteria_sum(), SEG_CRITERIA);
        assert_eq!(l.finding_totals(), (0, 0));
        let root = l.settled_value();
        assert_eq!(root, Some(l.fold()));
        assert_eq!(l.freeze(), root);
        assert_eq!(l.freeze(), root);
        assert!(l.is_frozen());
        assert_eq!(l.record(35, KIND_DRILL, 20, 0, 0), Err(BX_EFROZEN));
        assert_eq!(l.settle(), Err(BX_EFROZEN));
    }

    #[test]
    fn f11154_gates() {
        let mut l = DrillLedger::new();
        assert!(l.record(35, KIND_DRILL, 20, 0, 0).is_ok());
        assert!(l.record(36, KIND_ITYPE, 20, 0, 0).is_ok());
        assert_eq!((l.accepts, l.rejects), (2, 0));
        assert_eq!(l.record(34, KIND_DRILL, 20, 0, 0), Err(BX_ENO));
        assert_eq!(l.record(35, KIND_DRILL, 20, 0, 0), Err(BX_EDUP));
        assert_eq!(l.record(36, KIND_ITYPE, 19, 0, 0), Err(BX_EDUP));
        assert_eq!((l.accepts, l.rejects), (2, 3));

        let mut k = DrillLedger::new();
        assert_eq!(k.record(35, KIND_ITYPE, 20, 0, 0), Err(BX_EKIND));
        assert_eq!(k.record(35, KIND_DRILL, 20, 1, 3), Err(BX_EKIND));
        assert_eq!(k.record(36, KIND_DRILL, 20, 0, 0), Err(BX_EKIND));
        assert_eq!((k.accepts, k.rejects), (0, 3));

        let mut o = DrillLedger::new();
        let _ = o.record(36, KIND_ITYPE, 20, 0, 0);
        assert_eq!(o.record(35, KIND_DRILL, 20, 0, 0), Err(BX_EORDER));
        assert_eq!((o.accepts, o.rejects), (1, 1));

        let mut q = DrillLedger::new();
        let _ = q.record(35, KIND_DRILL, 20, 0, 0);
        assert_eq!(q.settle(), Err(BX_ESHORT));
        assert_eq!((q.accepts, q.rejects), (1, 1));

        let mut m = DrillLedger::new();
        let _ = m.record(35, KIND_DRILL, 19, 0, 0);
        let _ = m.record(36, KIND_ITYPE, 20, 0, 0);
        assert_eq!(m.settle(), Err(BX_ESUM));
        assert_eq!((m.accepts, m.rejects), (2, 1));

        let mut p = DrillLedger::new();
        let _ = p.record(35, KIND_DRILL, 20, 2, 1);
        let _ = p.record(36, KIND_ITYPE, 20, 0, 0);
        assert_eq!(p.settle(), Err(BX_EOPEN));
        assert_eq!((p.accepts, p.rejects), (2, 1));

        let (mut d, ok) = build_b3536();
        assert!(ok);
        assert!(d.freeze().is_some());
        assert_eq!(d.record(35, KIND_DRILL, 20, 0, 0), Err(BX_EFROZEN));
        assert_eq!(d.settle(), Err(BX_EFROZEN));
        assert_eq!((d.accepts, d.rejects), (2, 2));
    }

    #[test]
    fn f11154_checks_green() {
        assert!(run_unx11154_checks().all_passed());
    }
}
