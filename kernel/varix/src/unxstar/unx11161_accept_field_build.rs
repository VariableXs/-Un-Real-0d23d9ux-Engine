//! UNX-F11161 验收总核销场搭建（域 800 条验收账）（0x1f3d0 段）
//!
//! 纯功能模型：域验收场以 800 位位图承载 800 条判据的核销位——40 批、批批 20 位，
//! 批号定位位段。搭建要求批号 1..40 逐批到位、批内 20 位全核、全场无空洞；
//! 位图快照一经冻结，验收场不再接受任何补核销。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f3d0;
/// 批号越界（只认 B01–B40）
pub const AF_ENO: u32 = SEG_BASE + 0x01;
/// 批内核销位重复置位
pub const AF_EDUP: u32 = SEG_BASE + 0x02;
/// 批号倒序
pub const AF_EORDER: u32 = SEG_BASE + 0x03;
/// 批内核销数越界或为零（形状违规）
pub const AF_ECOUNT: u32 = SEG_BASE + 0x04;
/// 四十批未齐
pub const AF_ESHORT: u32 = SEG_BASE + 0x05;
/// 全场核销位不足 800
pub const AF_ESUM: u32 = SEG_BASE + 0x06;
/// 批内未满 20 位
pub const AF_EPART: u32 = SEG_BASE + 0x07;
/// 位图已冻结
pub const AF_EFROZEN: u32 = SEG_BASE + 0x08;

/// 域批数。
pub const BATCHES: usize = 40;
/// 每批判据数。
pub const PER_BATCH: usize = 20;
/// 域判据总数。
pub const TOTAL: usize = BATCHES * PER_BATCH;
/// 位图字数（13×64 = 832 ≥ 800）。
pub const WORDS: usize = 13;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 验收位图场。
#[derive(Clone, Debug)]
pub struct AcceptanceField {
    bits: [u64; WORDS],
    batches: u16,
    last: u8,
    settled: Option<u64>,
    frozen: bool,
    pub accepts: u64,
    pub rejects: u64,
}

impl AcceptanceField {
    /// 空场。
    pub fn new() -> AcceptanceField {
        AcceptanceField {
            bits: [0u64; WORDS],
            batches: 0,
            last: 0,
            settled: None,
            frozen: false,
            accepts: 0,
            rejects: 0,
        }
    }

    /// 已核销批数。
    pub fn batches(&self) -> u16 {
        self.batches
    }

    /// 是否空场。
    pub fn is_empty(&self) -> bool {
        self.batches == 0
    }

    /// 原位号（批号 1 基转 0 基）。
    fn bit_index(batch: usize, slot: usize) -> usize {
        batch
            .wrapping_sub(1)
            .wrapping_mul(PER_BATCH)
            .wrapping_add(slot)
    }

    /// 读某位。
    pub fn bit(&self, batch: usize, slot: usize) -> bool {
        if batch == 0 || batch > BATCHES || slot >= PER_BATCH {
            return false;
        }
        let idx = AcceptanceField::bit_index(batch, slot);
        let word = idx / 64;
        let off = idx % 64;
        match self.bits.get(word) {
            Some(w) => (w >> off) & 1 == 1,
            None => false,
        }
    }

    /// 某批已核销位数。
    pub fn batch_accepted(&self, batch: usize) -> u32 {
        let mut n = 0u32;
        let mut slot = 0usize;
        while slot < PER_BATCH {
            if self.bit(batch, slot) {
                n = n.wrapping_add(1);
            }
            slot = slot.wrapping_add(1);
        }
        n
    }

    /// 全场已核销位数。
    pub fn bits_set(&self) -> u32 {
        let mut n = 0u32;
        let mut b = 1usize;
        while b <= BATCHES {
            n = n.wrapping_add(self.batch_accepted(b));
            b = b.wrapping_add(1);
        }
        n
    }

    /// 核销一批：冻结 → 越界 → 重记 → 倒序 → 批内数形状。
    ///
    /// 重记先于倒序：同一批补核会撞上已置位，只报 AF_EDUP。
    pub fn record(&mut self, batch: usize, count: usize) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(AF_EFROZEN);
        }
        if batch == 0 || batch > BATCHES {
            return self.fail(AF_ENO);
        }
        if self.batch_accepted(batch) != 0 {
            return self.fail(AF_EDUP);
        }
        if self.last != 0 && batch <= self.last as usize {
            return self.fail(AF_EORDER);
        }
        if count == 0 || count > PER_BATCH {
            return self.fail(AF_ECOUNT);
        }
        let mut slot = 0usize;
        while slot < count {
            let idx = AcceptanceField::bit_index(batch, slot);
            let word = idx / 64;
            let off = idx % 64;
            if let Some(w) = self.bits.get_mut(word) {
                *w |= 1u64 << off;
            }
            slot = slot.wrapping_add(1);
        }
        self.batches = self.batches.wrapping_add(1);
        self.last = batch as u8;
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> Result<u64, u32> {
        self.rejects = self.rejects.wrapping_add(1);
        Err(code)
    }

    /// 满批表（未满 20 位的批号）。
    pub fn partial(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut b = 1usize;
        while b <= BATCHES {
            let n = self.batch_accepted(b);
            if n != 0 && n != PER_BATCH as u32 {
                out.push(b as u32);
            }
            b = b.wrapping_add(1);
        }
        out
    }

    /// 空洞位（未核销位）总数。
    pub fn holes(&self) -> u32 {
        (TOTAL as u32).wrapping_sub(self.bits_set())
    }

    /// 批号是否逐批到位。
    pub fn contiguous(&self) -> bool {
        if self.batches != BATCHES as u16 {
            return false;
        }
        let mut b = 1usize;
        while b <= BATCHES {
            if self.batch_accepted(b) == 0 {
                return false;
            }
            b = b.wrapping_add(1);
        }
        true
    }

    /// 满批核验（只读）：任一批未满 20 位即 AF_EPART。
    pub fn verify_full(&self) -> Result<(), u32> {
        if self.partial().is_empty() {
            Ok(())
        } else {
            Err(AF_EPART)
        }
    }

    /// 位图指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < WORDS {
            h = (h ^ self.bits[i]).wrapping_mul(FNV_PRIME);
            i = i.wrapping_add(1);
        }
        h = (h ^ self.batches as u64).wrapping_mul(FNV_PRIME);
        (h ^ self.last as u64).wrapping_mul(FNV_PRIME)
    }

    /// 验收场搭建：四十批 → 批批满位 → 全场 800 位。
    pub fn settle(&mut self) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(AF_EFROZEN);
        }
        if self.batches != BATCHES as u16 {
            return self.fail(AF_ESHORT);
        }
        if self.bits_set() != TOTAL as u32 || self.holes() != 0 {
            return self.fail(AF_ESUM);
        }
        let v = self.fold();
        self.settled = Some(v);
        Ok(v)
    }

    /// 冻结：锁位图快照，幂等。
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

/// 建场：40 批 × 20 位逐批置位，再核验满场。
pub fn build_field() -> (AcceptanceField, bool) {
    let mut f = AcceptanceField::new();
    let mut b = 1usize;
    while b <= BATCHES {
        if f.record(b, PER_BATCH).is_err() {
            return (f, false);
        }
        b = b.wrapping_add(1);
    }
    let ok = f.settle().is_ok() && f.verify_full().is_ok();
    (f, ok)
}

/// 四类注入：批号越界 / 位重复 / 批内数越界 / 冻结后补核。
pub fn inject_four() -> [bool; 4] {
    // ① 批号越界
    let mut a0 = AcceptanceField::new();
    let r0 = a0.record(0, 1).err() == Some(AF_ENO)
        && a0.record(BATCHES + 1, 1).err() == Some(AF_ENO)
        && a0.is_empty();

    // ② 位重复与倒序
    let mut a1 = AcceptanceField::new();
    let s0 = a1.record(1, PER_BATCH).is_ok();
    let s1 = a1.record(2, PER_BATCH).is_ok();
    let dup = s0 && s1 && a1.record(1, PER_BATCH).err() == Some(AF_EDUP);
    let mut a2 = AcceptanceField::new();
    let _ = a2.record(5, PER_BATCH);
    let order = a2.record(3, PER_BATCH).err() == Some(AF_EORDER);
    let r1 = dup && order;

    // ③ 批内核销数形状与全场不足
    let mut a3 = AcceptanceField::new();
    let shape = a3.record(1, 0).err() == Some(AF_ECOUNT)
        && a3.record(1, PER_BATCH + 1).err() == Some(AF_ECOUNT);
    let mut a4 = AcceptanceField::new();
    let mut b = 1usize;
    while b <= BATCHES {
        let n = if b == 9 { PER_BATCH - 1 } else { PER_BATCH };
        let _ = a4.record(b, n);
        b = b.wrapping_add(1);
    }
    let sum = a4.settle().err() == Some(AF_ESUM);
    let r2 = shape && sum;

    // ④ 冻结后补核
    let (mut a5, ok5) = build_field();
    let locked = a5.freeze().is_some();
    let r3 = ok5
        && locked
        && a5.record(1, 1).err() == Some(AF_EFROZEN)
        && a5.settle().err() == Some(AF_EFROZEN);

    [r0, r1, r2, r3]
}

/// 判据：UNX-F11161 验收总核销场搭建。
pub fn run_unx11161_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11161");
    let (mut f, ok) = build_field();

    // X1 四十批齐备
    s.add_dyn(
        "X1-四十批齐备",
        ok && f.batches() == BATCHES as u16,
        &format!("batches={}/{}", f.batches(), BATCHES),
    );

    // X2 批批满位
    let part = f.partial();
    s.add_dyn(
        "X2-批批满位",
        part.is_empty(),
        &format!("partial={} first={:?}", part.len(), part.first()),
    );

    // X3 全场 800 位
    s.add_dyn(
        "X3-全场八百位",
        f.bits_set() == TOTAL as u32,
        &format!("bits={}/{}", f.bits_set(), TOTAL),
    );

    // X4 零空洞
    s.add("X4-零空洞", f.holes() == 0, "800 位全部置位，无空洞");

    // X5 批号连续
    s.add("X5-批号连续", f.contiguous(), "B01–B40 逐批到位");

    // X6 位段端点
    s.add(
        "X6-位段端点",
        f.bit(1, 0) && f.bit(40, PER_BATCH - 1) && !f.bit(41, 0) && !f.bit(0, 0),
        "首端 B01#0 与末端 B40#19 置位，界外一律为假",
    );

    // X7 终值即指纹
    let root = f.settled_value();
    s.add("X7-终值即指纹", root == Some(f.fold()), "位图快照值等于 13 字指纹");

    // X8 冻结幂等
    let f1 = f.freeze();
    let f2 = f.freeze();
    s.add("X8-冻结幂等", f1.is_some() && f1 == f2, "二次冻结返回同一快照值");

    // X9 冻结后拒补核
    s.add(
        "X9-冻结后拒补核",
        f.is_frozen()
            && f.record(1, 1).err() == Some(AF_EFROZEN)
            && f.settle().err() == Some(AF_EFROZEN),
        "冻结后 record/settle 都落 AF_EFROZEN",
    );

    // X10 越界 / 重记 / 倒序
    let mut g = AcceptanceField::new();
    let g0 = g.record(1, PER_BATCH).is_ok();
    let mut o = AcceptanceField::new();
    let _ = o.record(5, PER_BATCH);
    s.add(
        "X10-越界重记倒序",
        g.record(0, 1).err() == Some(AF_ENO)
            && g.record(BATCHES + 1, 1).err() == Some(AF_ENO)
            && g0
            && g.record(1, PER_BATCH).err() == Some(AF_EDUP)
            && o.record(3, PER_BATCH).err() == Some(AF_EORDER),
        "越界 ENO、重记 EDUP、倒序 EORDER 各按其码",
    );

    // X11 批内数形状与满批核验
    let mut sh = AcceptanceField::new();
    let mut pf = AcceptanceField::new();
    let _ = pf.record(1, PER_BATCH);
    let _ = pf.record(2, PER_BATCH / 2);
    s.add(
        "X11-形状与满批核验",
        sh.record(1, 0).err() == Some(AF_ECOUNT)
            && sh.record(1, PER_BATCH + 1).err() == Some(AF_ECOUNT)
            && sh.is_empty()
            && pf.verify_full().err() == Some(AF_EPART),
        "零位/超位落 ECOUNT，半批核验落 EPART",
    );

    // X12 未齐 / 全场不足
    let mut q1 = AcceptanceField::new();
    let mut b = 1usize;
    while b < BATCHES {
        let _ = q1.record(b, PER_BATCH);
        b = b.wrapping_add(1);
    }
    let mut q2 = AcceptanceField::new();
    let mut b = 1usize;
    while b <= BATCHES {
        let n = if b == 17 { PER_BATCH - 2 } else { PER_BATCH };
        let _ = q2.record(b, n);
        b = b.wrapping_add(1);
    }
    let hits = inject_four();
    let all = hits[0] && hits[1] && hits[2] && hits[3];
    let (mut fresh, ok2) = build_field();
    let root2 = fresh.freeze().unwrap_or(0);
    s.add_dyn(
        "X13-两拒与注入",
        q1.settle().err() == Some(AF_ESHORT)
            && q2.settle().err() == Some(AF_ESUM)
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
    fn f11161_flow() {
        let (mut f, ok) = build_field();
        assert!(ok);
        assert_eq!(f.batches(), BATCHES as u16);
        assert!(f.partial().is_empty());
        assert_eq!(f.bits_set(), TOTAL as u32);
        assert_eq!(f.holes(), 0);
        assert!(f.contiguous());
        assert!(f.bit(1, 0));
        assert!(f.bit(40, PER_BATCH - 1));
        let root = f.settled_value();
        assert_eq!(root, Some(f.fold()));
        assert_eq!(f.freeze(), root);
        assert_eq!(f.freeze(), root);
        assert!(f.is_frozen());
        assert_eq!(f.record(1, 1), Err(AF_EFROZEN));
        assert_eq!(f.settle(), Err(AF_EFROZEN));
    }

    #[test]
    fn f11161_gates() {
        let mut l = AcceptanceField::new();
        assert!(l.record(1, PER_BATCH).is_ok());
        assert!(l.record(2, PER_BATCH).is_ok());
        assert_eq!((l.accepts, l.rejects), (2, 0));
        assert_eq!(l.record(BATCHES + 1, 1), Err(AF_ENO));
        assert_eq!(l.record(1, PER_BATCH), Err(AF_EDUP));
        assert_eq!((l.accepts, l.rejects), (2, 2));

        let mut o = AcceptanceField::new();
        let _ = o.record(5, PER_BATCH);
        assert_eq!(o.record(3, PER_BATCH), Err(AF_EORDER));
        assert_eq!((o.accepts, o.rejects), (1, 1));

        let mut c = AcceptanceField::new();
        assert_eq!(c.record(1, 0), Err(AF_ECOUNT));
        assert_eq!(c.record(1, PER_BATCH + 1), Err(AF_ECOUNT));
        assert_eq!((c.accepts, c.rejects), (0, 2));

        let mut q = AcceptanceField::new();
        let mut b = 1usize;
        while b < BATCHES {
            let _ = q.record(b, PER_BATCH);
            b = b.wrapping_add(1);
        }
        assert_eq!(q.settle(), Err(AF_ESHORT));
        assert_eq!((q.accepts, q.rejects), (39, 1));

        let mut m = AcceptanceField::new();
        let mut b = 1usize;
        while b <= BATCHES {
            let n = if b == 17 { PER_BATCH - 2 } else { PER_BATCH };
            let _ = m.record(b, n);
            b = b.wrapping_add(1);
        }
        assert_eq!(m.bits_set(), (TOTAL - 2) as u32);
        assert_eq!(m.settle(), Err(AF_ESUM));
        assert_eq!((m.accepts, m.rejects), (40, 1));

        let mut pf = AcceptanceField::new();
        let _ = pf.record(1, PER_BATCH);
        let _ = pf.record(2, PER_BATCH / 2);
        assert_eq!(pf.verify_full(), Err(AF_EPART));
        assert_eq!((pf.accepts, pf.rejects), (2, 0));

        let (mut d, ok) = build_field();
        assert!(ok);
        assert!(d.freeze().is_some());
        assert_eq!(d.record(1, 1), Err(AF_EFROZEN));
        assert_eq!(d.settle(), Err(AF_EFROZEN));
        assert_eq!((d.accepts, d.rejects), (40, 2));
    }

    #[test]
    fn f11161_checks_green() {
        assert!(run_unx11161_checks().all_passed());
    }
}
