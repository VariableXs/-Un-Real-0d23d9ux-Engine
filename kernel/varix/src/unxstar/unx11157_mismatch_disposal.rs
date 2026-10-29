//! UNX-F11157 清账段二失配处置账（终重放失配闭环）（0x1f390 段）
//!
//! 纯功能模型：段二终重放共暴露六例失配，逐例须归因到处置人（责任人非零）并
//! 落到已处置。闭环判据要求六例齐、三型失配全覆盖、在途归零；处置账冻结后
//! 段二不再新增失配，也不许把已处置例翻回在途。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f390;
/// 失配例号越界（只认 0..6）
pub const MD_ENO: u32 = SEG_BASE + 0x01;
/// 失配例重记
pub const MD_EDUP: u32 = SEG_BASE + 0x02;
/// 失配例序倒挂
pub const MD_EORDER: u32 = SEG_BASE + 0x03;
/// 失配型越界（形状违规）
pub const MD_EKIND: u32 = SEG_BASE + 0x04;
/// 六例未齐即闭环
pub const MD_ESHORT: u32 = SEG_BASE + 0x05;
/// 仍有在途失配
pub const MD_EOPEN: u32 = SEG_BASE + 0x06;
/// 处置人缺失或越界
pub const MD_EOWN: u32 = SEG_BASE + 0x07;
/// 终值已锁定
pub const MD_EFROZEN: u32 = SEG_BASE + 0x08;

/// 失配例数。
pub const CASES: usize = 6;
/// 失配型宽度。
pub const KINDS: u8 = 3;
/// 处置人下限。
pub const OWNER_MIN: u8 = 1;
/// 处置人上限。
pub const OWNER_MAX: u8 = 4;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一例失配处置件。
#[derive(Clone, Copy, Debug)]
pub struct Mismatch {
    pub id: u8,
    pub kind: u8,
    pub owner: u8,
    pub disposed: bool,
}

impl Mismatch {
    /// 本件是否闭环（有处置人且已处置）。
    pub fn closed(&self) -> bool {
        self.disposed && self.owner >= OWNER_MIN && self.owner <= OWNER_MAX
    }
}

/// 段二失配处置账。
#[derive(Clone, Debug)]
pub struct DisposalLedger {
    items: Vec<Mismatch>,
    settled: Option<u64>,
    frozen: bool,
    pub accepts: u64,
    pub rejects: u64,
}

impl DisposalLedger {
    /// 空账。
    pub fn new() -> DisposalLedger {
        DisposalLedger {
            items: Vec::new(),
            settled: None,
            frozen: false,
            accepts: 0,
            rejects: 0,
        }
    }

    /// 已记例数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 取某例。
    pub fn item(&self, id: usize) -> Option<Mismatch> {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].id as usize == id {
                return Some(self.items[i]);
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 记一例：冻结 → 越界 → 重记 → 倒序 → 失配型 → 处置人形状。
    ///
    /// 重记先于倒序；失配型先于处置人，两路形状各自可达。
    pub fn record(&mut self, id: usize, kind: u8, owner: u8, disposed: bool) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(MD_EFROZEN);
        }
        if id >= CASES {
            return self.fail(MD_ENO);
        }
        if self.item(id).is_some() {
            return self.fail(MD_EDUP);
        }
        if let Some(last) = self.items.last() {
            if id <= last.id as usize {
                return self.fail(MD_EORDER);
            }
        }
        if kind >= KINDS {
            return self.fail(MD_EKIND);
        }
        if owner < OWNER_MIN || owner > OWNER_MAX {
            return self.fail(MD_EOWN);
        }
        self.items.push(Mismatch {
            id: id as u8,
            kind,
            owner,
            disposed,
        });
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> Result<u64, u32> {
        self.rejects = self.rejects.wrapping_add(1);
        Err(code)
    }

    /// 在途例数。
    pub fn open_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            if !self.items[i].disposed {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 已处置例数。
    pub fn disposed_count(&self) -> u32 {
        (self.items.len() as u32).wrapping_sub(self.open_count())
    }

    /// 处置人分布（1..=4 各计）。
    pub fn owner_histogram(&self) -> [u32; OWNER_MAX as usize] {
        let mut h = [0u32; OWNER_MAX as usize];
        let mut i = 0usize;
        while i < self.items.len() {
            let o = self.items[i].owner;
            if o >= OWNER_MIN && o <= OWNER_MAX {
                let slot = (o - OWNER_MIN) as usize;
                if let Some(v) = h.get_mut(slot) {
                    *v = v.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        h
    }

    /// 三型覆盖：每型至少一例。
    pub fn kinds_covered(&self) -> bool {
        let mut seen = [false; KINDS as usize];
        let mut i = 0usize;
        while i < self.items.len() {
            let k = self.items[i].kind as usize;
            if let Some(v) = seen.get_mut(k) {
                *v = true;
            }
            i = i.wrapping_add(1);
        }
        let mut j = 0usize;
        while j < seen.len() {
            if !seen[j] {
                return false;
            }
            j = j.wrapping_add(1);
        }
        true
    }

    /// 缺例表。
    pub fn gaps(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut id = 0usize;
        while id < CASES {
            if self.item(id).is_none() {
                out.push(id as u32);
            }
            id = id.wrapping_add(1);
        }
        out
    }

    /// 例序严格。
    pub fn order_strict(&self) -> bool {
        let mut i = 1usize;
        while i < self.items.len() {
            if self.items[i].id <= self.items[i - 1].id {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 处置账指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.items.len() {
            let m = self.items[i];
            h = (h ^ m.id as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ m.kind as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ m.owner as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ m.disposed as u64).wrapping_mul(FNV_PRIME);
            i = i.wrapping_add(1);
        }
        h = (h ^ self.items.len() as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.open_count() as u64).wrapping_mul(FNV_PRIME);
        h
    }

    /// 闭环：六例齐 → 三型覆盖 → 在途归零 → 逐例闭合。
    pub fn settle(&mut self) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(MD_EFROZEN);
        }
        if self.items.len() != CASES || !self.gaps().is_empty() {
            return self.fail(MD_ESHORT);
        }
        if !self.kinds_covered() {
            return self.fail(MD_EKIND);
        }
        if self.open_count() != 0 {
            return self.fail(MD_EOPEN);
        }
        let mut i = 0usize;
        while i < self.items.len() {
            if !self.items[i].closed() {
                return self.fail(MD_EOPEN);
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

/// 建账：六例失配按三型覆盖、四名处置人分担、全部已处置，再闭环。
pub fn build_ledger() -> (DisposalLedger, bool) {
    let mut l = DisposalLedger::new();
    let mut id = 0usize;
    while id < CASES {
        let kind = (id % KINDS as usize) as u8;
        let owner = ((id % OWNER_MAX as usize) + 1) as u8;
        if l.record(id, kind, owner, true).is_err() {
            return (l, false);
        }
        id = id.wrapping_add(1);
    }
    let ok = l.settle().is_ok();
    (l, ok)
}

/// 四类注入：例号越界 / 重记与倒序 / 失配型越界 / 终账后再记。
pub fn inject_four() -> [bool; 4] {
    // ① 例号越界
    let mut a0 = DisposalLedger::new();
    let r0 = a0.record(CASES, 0, 1, true).err() == Some(MD_ENO)
        && a0.record(CASES.wrapping_add(4), 1, 2, true).err() == Some(MD_ENO)
        && a0.is_empty();

    // ② 重记与倒序
    let mut a1 = DisposalLedger::new();
    let s0 = a1.record(0, 0, 1, true).is_ok();
    let s1 = a1.record(1, 1, 2, true).is_ok();
    let dup = s0 && s1 && a1.record(0, 0, 1, true).err() == Some(MD_EDUP);
    let mut a2 = DisposalLedger::new();
    let _ = a2.record(3, 0, 1, true);
    let order = a2.record(2, 0, 1, true).err() == Some(MD_EORDER);
    let r1 = dup && order;

    // ③ 失配型越界与处置人缺失
    let mut a3 = DisposalLedger::new();
    let shape = a3.record(0, KINDS, 1, true).err() == Some(MD_EKIND)
        && a3.record(0, 0, 0, true).err() == Some(MD_EOWN)
        && a3.record(0, 0, OWNER_MAX.wrapping_add(1), true).err() == Some(MD_EOWN);
    let mut a4 = DisposalLedger::new();
    let mut id = 0usize;
    while id < CASES {
        let _ = a4.record(id, (id % KINDS as usize) as u8, 1, id != 2);
        id = id.wrapping_add(1);
    }
    let open = a4.settle().err() == Some(MD_EOPEN);
    let r2 = shape && open;

    // ④ 终账后再记 / 再闭环
    let (mut a5, ok5) = build_ledger();
    let locked = a5.freeze().is_some();
    let r3 = ok5
        && locked
        && a5.record(0, 0, 1, true).err() == Some(MD_EFROZEN)
        && a5.settle().err() == Some(MD_EFROZEN);

    [r0, r1, r2, r3]
}

/// 判据：UNX-F11157 清账段二失配处置账。
pub fn run_unx11157_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11157");
    let (mut l, ok) = build_ledger();

    // X1 六例齐备
    s.add_dyn(
        "X1-六例齐备",
        ok && l.len() == CASES && l.gaps().is_empty(),
        &format!("cases={}/{} gaps={}", l.len(), CASES, l.gaps().len()),
    );

    // X2 三型覆盖
    s.add("X2-三型覆盖", l.kinds_covered(), "三类失配各至少一例");

    // X3 在途归零
    s.add_dyn(
        "X3-在途归零",
        l.open_count() == 0 && l.disposed_count() == CASES as u32,
        &format!("open={} disposed={}", l.open_count(), l.disposed_count()),
    );

    // X4 处置人分布
    let hist = l.owner_histogram();
    s.add_dyn(
        "X4-处置人分布",
        hist.len() == 4 && hist.iter().all(|v| *v > 0),
        &format!("owners={:?}", hist),
    );

    // X5 例序严格
    s.add("X5-例序严格", l.order_strict(), "六例按 0..5 递增入账");

    // X6 终值即指纹
    let root = l.settled_value();
    s.add("X6-终值即指纹", root == Some(l.fold()), "处置清单值等于逐例指纹");

    // X7 冻结幂等
    let f1 = l.freeze();
    let f2 = l.freeze();
    s.add("X7-冻结幂等", f1.is_some() && f1 == f2, "二次冻结返回同一单值");

    // X8 冻结后拒记拒闭环
    s.add(
        "X8-冻结后拒记拒闭",
        l.is_frozen()
            && l.record(0, 0, 1, true).err() == Some(MD_EFROZEN)
            && l.settle().err() == Some(MD_EFROZEN),
        "冻结后 record/settle 都落 MD_EFROZEN",
    );

    // X9 例号越界拒止
    let mut g = DisposalLedger::new();
    s.add(
        "X9-例号越界拒止",
        g.record(CASES, 0, 1, true).err() == Some(MD_ENO)
            && g.record(CASES.wrapping_add(2), 0, 1, true).err() == Some(MD_ENO)
            && g.is_empty(),
        "0..6 之外的失配例号一律 MD_ENO",
    );

    // X10 重记先于倒序
    let mut d = DisposalLedger::new();
    let _ = d.record(0, 0, 1, true);
    let _ = d.record(1, 1, 2, true);
    let mut o = DisposalLedger::new();
    let _ = o.record(3, 0, 1, true);
    s.add(
        "X10-重记先于倒序",
        d.record(0, 0, 1, true).err() == Some(MD_EDUP)
            && o.record(2, 0, 1, true).err() == Some(MD_EORDER),
        "同例重记报 EDUP，真倒序才报 EORDER",
    );

    // X11 失配型与处置人拒止
    let mut sh = DisposalLedger::new();
    s.add(
        "X11-型与处置人拒止",
        sh.record(0, KINDS, 1, true).err() == Some(MD_EKIND)
            && sh.record(0, 0, 0, true).err() == Some(MD_EOWN)
            && sh.record(0, 0, OWNER_MAX.wrapping_add(1), true).err() == Some(MD_EOWN)
            && sh.is_empty(),
        "失配型 ≥3 落 EKIND，处置人越界落 EOWN",
    );

    // X12 未齐 / 三型缺覆盖 / 在途
    let mut q1 = DisposalLedger::new();
    let mut id = 0usize;
    while id < CASES - 1 {
        let _ = q1.record(id, (id % KINDS as usize) as u8, 1, true);
        id = id.wrapping_add(1);
    }
    let mut q2 = DisposalLedger::new();
    let mut id = 0usize;
    while id < CASES {
        let _ = q2.record(id, 0, 1, true);
        id = id.wrapping_add(1);
    }
    let mut q3 = DisposalLedger::new();
    let mut id = 0usize;
    while id < CASES {
        let _ = q3.record(id, (id % KINDS as usize) as u8, 1, id != 4);
        id = id.wrapping_add(1);
    }
    let hits = inject_four();
    let all = hits[0] && hits[1] && hits[2] && hits[3];
    let (mut fresh, ok2) = build_ledger();
    let root2 = fresh.freeze().unwrap_or(0);
    s.add_dyn(
        "X12-三拒与注入",
        q1.settle().err() == Some(MD_ESHORT)
            && q2.settle().err() == Some(MD_EKIND)
            && q3.settle().err() == Some(MD_EOPEN)
            && all
            && ok2
            && fresh.accepts == CASES as u64
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
    fn f11157_flow() {
        let (mut l, ok) = build_ledger();
        assert!(ok);
        assert_eq!(l.len(), CASES);
        assert!(l.gaps().is_empty());
        assert!(l.order_strict());
        assert!(l.kinds_covered());
        assert_eq!(l.open_count(), 0);
        assert_eq!(l.disposed_count(), CASES as u32);
        let root = l.settled_value();
        assert_eq!(root, Some(l.fold()));
        assert_eq!(l.freeze(), root);
        assert_eq!(l.freeze(), root);
        assert!(l.is_frozen());
        assert_eq!(l.record(0, 0, 1, true), Err(MD_EFROZEN));
        assert_eq!(l.settle(), Err(MD_EFROZEN));
    }

    #[test]
    fn f11157_gates() {
        let mut l = DisposalLedger::new();
        assert!(l.record(0, 0, 1, true).is_ok());
        assert!(l.record(1, 1, 2, true).is_ok());
        assert_eq!((l.accepts, l.rejects), (2, 0));
        assert_eq!(l.record(CASES, 0, 1, true), Err(MD_ENO));
        assert_eq!(l.record(0, 0, 1, true), Err(MD_EDUP));
        assert_eq!((l.accepts, l.rejects), (2, 2));

        let mut k = DisposalLedger::new();
        assert_eq!(k.record(0, KINDS, 1, true), Err(MD_EKIND));
        assert_eq!(k.record(0, 0, 0, true), Err(MD_EOWN));
        assert_eq!(k.record(0, 0, OWNER_MAX.wrapping_add(1), true), Err(MD_EOWN));
        assert_eq!((k.accepts, k.rejects), (0, 3));

        let mut o = DisposalLedger::new();
        let _ = o.record(3, 0, 1, true);
        assert_eq!(o.record(2, 0, 1, true), Err(MD_EORDER));
        assert_eq!((o.accepts, o.rejects), (1, 1));

        let mut sh = DisposalLedger::new();
        let mut id = 0usize;
        while id < CASES - 1 {
            let _ = sh.record(id, (id % KINDS as usize) as u8, 1, true);
            id = id.wrapping_add(1);
        }
        assert_eq!(sh.settle(), Err(MD_ESHORT));
        assert_eq!((sh.accepts, sh.rejects), (5, 1));

        let mut kc = DisposalLedger::new();
        let mut id = 0usize;
        while id < CASES {
            let _ = kc.record(id, 0, 1, true);
            id = id.wrapping_add(1);
        }
        assert_eq!(kc.settle(), Err(MD_EKIND));
        assert_eq!((kc.accepts, kc.rejects), (6, 1));

        let mut op = DisposalLedger::new();
        let mut id = 0usize;
        while id < CASES {
            let _ = op.record(id, (id % KINDS as usize) as u8, 1, id != 4);
            id = id.wrapping_add(1);
        }
        assert_eq!(op.open_count(), 1);
        assert_eq!(op.settle(), Err(MD_EOPEN));
        assert_eq!((op.accepts, op.rejects), (6, 1));

        let (mut d, ok) = build_ledger();
        assert!(ok);
        assert!(d.freeze().is_some());
        assert_eq!(d.record(0, 0, 1, true), Err(MD_EFROZEN));
        assert_eq!(d.settle(), Err(MD_EFROZEN));
        assert_eq!((d.accepts, d.rejects), (6, 2));
    }

    #[test]
    fn f11157_checks_green() {
        assert!(run_unx11157_checks().all_passed());
    }
}
