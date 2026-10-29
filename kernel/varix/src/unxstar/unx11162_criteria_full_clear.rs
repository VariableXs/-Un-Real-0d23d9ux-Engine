//! UNX-F11162 域 800 条判据全量核销对账（0x1f3e0 段）
//!
//! 纯功能模型：域 800 条判据按 40 批核销，每批报「核销数 + 重复数」。全量对账
//! 要求四十批齐、批批核销与重复相加不超 20、全场重复为零、核销总数恰 800，
//! 核销率因此恒为 100%；对账单值冻结后不再收核销件。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f3e0;
/// 批号越界（只认 B01–B40）
pub const CL_ENO: u32 = SEG_BASE + 0x01;
/// 批重复对账
pub const CL_EDUP: u32 = SEG_BASE + 0x02;
/// 批序倒挂
pub const CL_EORDER: u32 = SEG_BASE + 0x03;
/// 核销数与重复数越界（形状违规）
pub const CL_EBAL: u32 = SEG_BASE + 0x04;
/// 四十批未齐
pub const CL_ESHORT: u32 = SEG_BASE + 0x05;
/// 核销总数不足 800
pub const CL_ESUM: u32 = SEG_BASE + 0x06;
/// 终值已锁定
pub const CL_EFROZEN: u32 = SEG_BASE + 0x07;

/// 域批数。
pub const BATCHES: usize = 40;
/// 每批判据数。
pub const PER_BATCH: u32 = 20;
/// 域判据总数。
pub const TOTAL: u32 = BATCHES as u32 * PER_BATCH;
/// 核销率满值（千分比）。
pub const RATE_FULL: u32 = 1_000;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一批核销件。
#[derive(Clone, Copy, Debug)]
pub struct ClearTicket {
    pub batch: u8,
    pub cleared: u16,
    pub dups: u16,
}

impl ClearTicket {
    /// 本批账面件数（核销 + 重复）。
    pub fn accounted(&self) -> u32 {
        self.cleared as u32 + self.dups as u32
    }

    /// 本批是否干净（无重复且核销满 20）。
    pub fn clean(&self) -> bool {
        self.dups == 0 && self.cleared as u32 == PER_BATCH
    }
}

/// 域判据核销对账簿。
#[derive(Clone, Debug)]
pub struct ClearBook {
    tickets: Vec<ClearTicket>,
    settled: Option<u64>,
    frozen: bool,
    pub accepts: u64,
    pub rejects: u64,
}

impl ClearBook {
    /// 空账。
    pub fn new() -> ClearBook {
        ClearBook {
            tickets: Vec::new(),
            settled: None,
            frozen: false,
            accepts: 0,
            rejects: 0,
        }
    }

    /// 已记批数。
    pub fn len(&self) -> usize {
        self.tickets.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.tickets.is_empty()
    }

    /// 取某批。
    pub fn ticket(&self, batch: usize) -> Option<ClearTicket> {
        let mut i = 0usize;
        while i < self.tickets.len() {
            if self.tickets[i].batch as usize == batch {
                return Some(self.tickets[i]);
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 记一批：冻结 → 越界 → 重记 → 倒序 → 账面越界。
    ///
    /// 重记先于倒序；账面越界单独报 CL_EBAL，不吞重记。
    pub fn record(&mut self, batch: usize, cleared: u16, dups: u16) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(CL_EFROZEN);
        }
        if batch == 0 || batch > BATCHES {
            return self.fail(CL_ENO);
        }
        if self.ticket(batch).is_some() {
            return self.fail(CL_EDUP);
        }
        if let Some(last) = self.tickets.last() {
            if batch <= last.batch as usize {
                return self.fail(CL_EORDER);
            }
        }
        let sum = cleared as u32 + dups as u32;
        if cleared as u32 > PER_BATCH || dups as u32 > PER_BATCH || sum > PER_BATCH {
            return self.fail(CL_EBAL);
        }
        self.tickets.push(ClearTicket {
            batch: batch as u8,
            cleared,
            dups,
        });
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> Result<u64, u32> {
        self.rejects = self.rejects.wrapping_add(1);
        Err(code)
    }

    /// 核销总数。
    pub fn total_cleared(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < self.tickets.len() {
            t = t.wrapping_add(self.tickets[i].cleared as u32);
            i = i.wrapping_add(1);
        }
        t
    }

    /// 重复总数。
    pub fn total_dups(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < self.tickets.len() {
            t = t.wrapping_add(self.tickets[i].dups as u32);
            i = i.wrapping_add(1);
        }
        t
    }

    /// 账面件数合计。
    pub fn total_accounted(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < self.tickets.len() {
            t = t.wrapping_add(self.tickets[i].accounted());
            i = i.wrapping_add(1);
        }
        t
    }

    /// 核销率（千分比，基于 800 条分母）。
    pub fn rate_permille(&self) -> u32 {
        self.total_cleared()
            .wrapping_mul(1_000)
            .wrapping_div(TOTAL)
    }

    /// 缺批表。
    pub fn gaps(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut b = 1usize;
        while b <= BATCHES {
            if self.ticket(b).is_none() {
                out.push(b as u32);
            }
            b = b.wrapping_add(1);
        }
        out
    }

    /// 批序严格。
    pub fn order_strict(&self) -> bool {
        let mut i = 1usize;
        while i < self.tickets.len() {
            if self.tickets[i].batch <= self.tickets[i - 1].batch {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 批批干净。
    pub fn all_clean(&self) -> bool {
        let mut i = 0usize;
        while i < self.tickets.len() {
            if !self.tickets[i].clean() {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 核销账簿指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.tickets.len() {
            let t = self.tickets[i];
            h = (h ^ t.batch as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ t.cleared as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ t.dups as u64).wrapping_mul(FNV_PRIME);
            i = i.wrapping_add(1);
        }
        h = (h ^ self.tickets.len() as u64).wrapping_mul(FNV_PRIME);
        (h ^ self.total_cleared() as u64).wrapping_mul(FNV_PRIME)
    }

    /// 全量对账：四十批齐 → 无重复 → 核销满 800 → 批批干净。
    pub fn settle(&mut self) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(CL_EFROZEN);
        }
        if self.tickets.len() != BATCHES || !self.gaps().is_empty() {
            return self.fail(CL_ESHORT);
        }
        if self.total_dups() != 0 {
            return self.fail(CL_EDUP);
        }
        if self.total_cleared() != TOTAL {
            return self.fail(CL_ESUM);
        }
        if !self.all_clean() || self.rate_permille() != RATE_FULL {
            return self.fail(CL_ESUM);
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

/// 建账：40 批各核销 20 条、零重复，再全量对账。
pub fn build_book() -> (ClearBook, bool) {
    let mut b = ClearBook::new();
    let mut i = 1usize;
    while i <= BATCHES {
        if b.record(i, PER_BATCH as u16, 0).is_err() {
            return (b, false);
        }
        i = i.wrapping_add(1);
    }
    let ok = b.settle().is_ok();
    (b, ok)
}

/// 四类注入：批号越界 / 重批与倒序 / 账面越界 / 终账后再记。
pub fn inject_four() -> [bool; 4] {
    // ① 批号越界
    let mut a0 = ClearBook::new();
    let r0 = a0.record(0, 1, 0).err() == Some(CL_ENO)
        && a0.record(BATCHES + 1, 1, 0).err() == Some(CL_ENO)
        && a0.is_empty();

    // ② 重批与倒序
    let mut a1 = ClearBook::new();
    let s0 = a1.record(1, PER_BATCH as u16, 0).is_ok();
    let s1 = a1.record(2, PER_BATCH as u16, 0).is_ok();
    let dup = s0 && s1 && a1.record(1, PER_BATCH as u16, 0).err() == Some(CL_EDUP);
    let mut a2 = ClearBook::new();
    let _ = a2.record(5, PER_BATCH as u16, 0);
    let order = a2.record(3, PER_BATCH as u16, 0).err() == Some(CL_EORDER);
    let r1 = dup && order;

    // ③ 账面越界与重复核销
    let mut a3 = ClearBook::new();
    let bal = a3
        .record(1, (PER_BATCH + 1) as u16, 0)
        .err()
        == Some(CL_EBAL)
        && a3.record(1, 0, (PER_BATCH + 1) as u16).err() == Some(CL_EBAL)
        && a3
            .record(1, (PER_BATCH - 1) as u16, 2)
            .err()
            == Some(CL_EBAL);
    let mut a4 = ClearBook::new();
    let mut i = 1usize;
    while i <= BATCHES {
        let dups = if i == 6 { 1 } else { 0 };
        let cleared = PER_BATCH as u16 - dups;
        let _ = a4.record(i, cleared, dups);
        i = i.wrapping_add(1);
    }
    let dup_total = a4.settle().err() == Some(CL_EDUP);
    let r2 = bal && dup_total;

    // ④ 终账后再记 / 再对账
    let (mut a5, ok5) = build_book();
    let locked = a5.freeze().is_some();
    let r3 = ok5
        && locked
        && a5.record(1, 1, 0).err() == Some(CL_EFROZEN)
        && a5.settle().err() == Some(CL_EFROZEN);

    [r0, r1, r2, r3]
}

/// 判据：UNX-F11162 域 800 条判据全量核销对账。
pub fn run_unx11162_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11162");
    let (mut b, ok) = build_book();

    // X1 四十批齐备
    s.add_dyn(
        "X1-四十批齐备",
        ok && b.len() == BATCHES && b.gaps().is_empty(),
        &format!("batches={}/{} gaps={}", b.len(), BATCHES, b.gaps().len()),
    );

    // X2 核销满八百
    s.add_dyn(
        "X2-核销满八百",
        b.total_cleared() == TOTAL,
        &format!("cleared={}/{}", b.total_cleared(), TOTAL),
    );

    // X3 重复归零
    s.add_dyn(
        "X3-重复归零",
        b.total_dups() == 0,
        &format!("dups={}", b.total_dups()),
    );

    // X4 核销率满值
    s.add_dyn(
        "X4-核销率满值",
        b.rate_permille() == RATE_FULL,
        &format!("rate={}/{}", b.rate_permille(), RATE_FULL),
    );

    // X5 批批干净
    s.add_dyn(
        "X5-批批干净",
        b.all_clean() && b.total_accounted() == TOTAL,
        &format!("accounted={}/{}", b.total_accounted(), TOTAL),
    );

    // X6 批序严格
    s.add("X6-批序严格", b.order_strict(), "B01–B40 逐批递增");

    // X7 终值即指纹
    let root = b.settled_value();
    s.add("X7-终值即指纹", root == Some(b.fold()), "对账单值等于四十批指纹");

    // X8 冻结幂等
    let f1 = b.freeze();
    let f2 = b.freeze();
    s.add("X8-冻结幂等", f1.is_some() && f1 == f2, "二次冻结返回同一单值");

    // X9 冻结后拒记拒清
    s.add(
        "X9-冻结后拒记拒清",
        b.is_frozen()
            && b.record(1, 1, 0).err() == Some(CL_EFROZEN)
            && b.settle().err() == Some(CL_EFROZEN),
        "冻结后 record/settle 都落 CL_EFROZEN",
    );

    // X10 越界 / 重批 / 倒序
    let mut g = ClearBook::new();
    let g0 = g.record(1, PER_BATCH as u16, 0).is_ok();
    let mut o = ClearBook::new();
    let _ = o.record(5, PER_BATCH as u16, 0);
    s.add(
        "X10-越界重批倒序",
        g.record(0, 1, 0).err() == Some(CL_ENO)
            && g.record(BATCHES + 1, 1, 0).err() == Some(CL_ENO)
            && g0
            && g.record(1, 1, 0).err() == Some(CL_EDUP)
            && o.record(3, 1, 0).err() == Some(CL_EORDER),
        "越界 ENO、重批 EDUP、倒序 EORDER 各按其码",
    );

    // X11 账面越界
    let mut sh = ClearBook::new();
    s.add(
        "X11-账面越界拒止",
        sh.record(1, (PER_BATCH + 1) as u16, 0).err() == Some(CL_EBAL)
            && sh.record(1, 0, (PER_BATCH + 1) as u16).err() == Some(CL_EBAL)
            && sh.record(1, (PER_BATCH - 1) as u16, 2).err() == Some(CL_EBAL)
            && sh.is_empty(),
        "单计越 20 或合计越 20 都落 CL_EBAL",
    );

    // X12 未齐 / 重复未清 / 核销不足
    let mut q1 = ClearBook::new();
    let mut i = 1usize;
    while i < BATCHES {
        let _ = q1.record(i, PER_BATCH as u16, 0);
        i = i.wrapping_add(1);
    }
    let mut q2 = ClearBook::new();
    let mut i = 1usize;
    while i <= BATCHES {
        let dups = if i == 12 { 1 } else { 0 };
        let _ = q2.record(i, PER_BATCH as u16 - dups, dups);
        i = i.wrapping_add(1);
    }
    let mut q3 = ClearBook::new();
    let mut i = 1usize;
    while i <= BATCHES {
        let cleared = if i == 20 {
            PER_BATCH as u16 - 1
        } else {
            PER_BATCH as u16
        };
        let _ = q3.record(i, cleared, 0);
        i = i.wrapping_add(1);
    }
    let hits = inject_four();
    let all = hits[0] && hits[1] && hits[2] && hits[3];
    let (mut fresh, ok2) = build_book();
    let root2 = fresh.freeze().unwrap_or(0);
    s.add_dyn(
        "X12-三拒与注入",
        q1.settle().err() == Some(CL_ESHORT)
            && q2.settle().err() == Some(CL_EDUP)
            && q3.settle().err() == Some(CL_ESUM)
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
    fn f11162_flow() {
        let (mut b, ok) = build_book();
        assert!(ok);
        assert_eq!(b.len(), BATCHES);
        assert!(b.gaps().is_empty());
        assert!(b.order_strict());
        assert!(b.all_clean());
        assert_eq!(b.total_cleared(), TOTAL);
        assert_eq!(b.total_dups(), 0);
        assert_eq!(b.rate_permille(), RATE_FULL);
        let root = b.settled_value();
        assert_eq!(root, Some(b.fold()));
        assert_eq!(b.freeze(), root);
        assert_eq!(b.freeze(), root);
        assert!(b.is_frozen());
        assert_eq!(b.record(1, 1, 0), Err(CL_EFROZEN));
        assert_eq!(b.settle(), Err(CL_EFROZEN));
    }

    #[test]
    fn f11162_gates() {
        let mut l = ClearBook::new();
        assert!(l.record(1, PER_BATCH as u16, 0).is_ok());
        assert!(l.record(2, PER_BATCH as u16, 0).is_ok());
        assert_eq!((l.accepts, l.rejects), (2, 0));
        assert_eq!(l.record(BATCHES + 1, 1, 0), Err(CL_ENO));
        assert_eq!(l.record(1, 1, 0), Err(CL_EDUP));
        assert_eq!((l.accepts, l.rejects), (2, 2));

        let mut bal = ClearBook::new();
        assert_eq!(bal.record(1, (PER_BATCH + 1) as u16, 0), Err(CL_EBAL));
        assert_eq!(bal.record(1, 0, (PER_BATCH + 1) as u16), Err(CL_EBAL));
        assert_eq!(bal.record(1, (PER_BATCH - 1) as u16, 2), Err(CL_EBAL));
        assert_eq!((bal.accepts, bal.rejects), (0, 3));

        let mut o = ClearBook::new();
        let _ = o.record(5, PER_BATCH as u16, 0);
        assert_eq!(o.record(3, PER_BATCH as u16, 0), Err(CL_EORDER));
        assert_eq!((o.accepts, o.rejects), (1, 1));

        let mut q = ClearBook::new();
        let mut i = 1usize;
        while i < BATCHES {
            let _ = q.record(i, PER_BATCH as u16, 0);
            i = i.wrapping_add(1);
        }
        assert_eq!(q.settle(), Err(CL_ESHORT));
        assert_eq!((q.accepts, q.rejects), (39, 1));

        let mut d = ClearBook::new();
        let mut i = 1usize;
        while i <= BATCHES {
            let dups = if i == 12 { 1 } else { 0 };
            let _ = d.record(i, PER_BATCH as u16 - dups, dups);
            i = i.wrapping_add(1);
        }
        assert_eq!(d.settle(), Err(CL_EDUP));
        assert_eq!((d.accepts, d.rejects), (40, 1));

        let mut m = ClearBook::new();
        let mut i = 1usize;
        while i <= BATCHES {
            let cleared = if i == 20 {
                PER_BATCH as u16 - 1
            } else {
                PER_BATCH as u16
            };
            let _ = m.record(i, cleared, 0);
            i = i.wrapping_add(1);
        }
        assert_eq!(m.settle(), Err(CL_ESUM));
        assert_eq!((m.accepts, m.rejects), (40, 1));

        let (mut e, ok) = build_book();
        assert!(ok);
        assert!(e.freeze().is_some());
        assert_eq!(e.record(1, 1, 0), Err(CL_EFROZEN));
        assert_eq!(e.settle(), Err(CL_EFROZEN));
        assert_eq!((e.accepts, e.rejects), (40, 2));
    }

    #[test]
    fn f11162_checks_green() {
        assert!(run_unx11162_checks().all_passed());
    }
}
