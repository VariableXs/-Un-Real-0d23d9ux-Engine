//! UNX-F11194 域闭账断言一（800/800 满条单值）（0x1f5f0 段）
//!
//! 锚点：UNX-F11194-J1 满条断言单值成立（10/10），断言账版本冻结（10/10）。
//!
//! 纯功能模型：域内四十批、每批二十条判据。逐批开账 → 逐条入册 → 批封 →
//! 全批封齐后汇出「满条单值」。批号 1..=40 连续、批内条号 1..=20 逐条递增、
//! 总条数恰为 800，三条同时成立才允许出单值；单值一经冻结即锁死，重复冻结
//! 取回原值（绝不按当下账面重算），冻结后任何入册/封批/求和一律 FROZEN 拒止。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f5f0;
/// 批号越界（合法域 1..=40）
pub const DC_EBATCH: u32 = SEG_BASE + 0x01;
/// 条号越界（合法域 1..=20）
pub const DC_ESLOT: u32 = SEG_BASE + 0x02;
/// 条号跳号（须自 1 起逐条递增）
pub const DC_ESEQ: u32 = SEG_BASE + 0x03;
/// 批次未满二十条即封
pub const DC_ESHORT: u32 = SEG_BASE + 0x04;
/// 冻结后改写
pub const DC_EFROZEN: u32 = SEG_BASE + 0x05;
/// 未满 800 条先求单值
pub const DC_EUNFULL: u32 = SEG_BASE + 0x06;
/// 重复求单值
pub const DC_EDUP: u32 = SEG_BASE + 0x07;
/// 重复封批
pub const DC_ESEALED: u32 = SEG_BASE + 0x08;

/// 域内批数（B01–B40）。
pub const BATCHES: usize = 40;
/// 批内条数（每批二十条）。
pub const PER_BATCH: u16 = 20;
/// 满条总数（40 × 20 = 800）。
pub const TOTAL: u32 = 800;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 单批断言账：已入册条数、条序指纹、封批位。
#[derive(Clone, Debug, PartialEq)]
pub struct BatchTally {
    pub no: u16,
    pub filled: u16,
    digest: u64,
    pub sealed: bool,
}

impl BatchTally {
    fn new(no: u16) -> BatchTally {
        let seed = FNV_OFF ^ (no as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        BatchTally { no, filled: 0, digest: seed, sealed: false }
    }

    /// 条号合法域：1..=20。
    pub fn slot_legal(slot: u16) -> bool {
        slot >= 1 && slot <= PER_BATCH
    }

    /// 批内条号是否恰满。
    pub fn full(&self) -> bool {
        self.filled == PER_BATCH
    }

    /// 入册一条：封批拒、条号越界拒、跳号拒。条序严格递增是本卡不变式。
    pub fn enroll(&mut self, slot: u16, tag: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(DC_EFROZEN);
        }
        if !BatchTally::slot_legal(slot) {
            return Err(DC_ESLOT);
        }
        if slot != self.filled.wrapping_add(1) {
            return Err(DC_ESEQ);
        }
        self.filled = self.filled.wrapping_add(1);
        self.digest = (self.digest ^ tag as u64).wrapping_mul(FNV_PRIME);
        self.digest = (self.digest ^ slot as u64).wrapping_mul(FNV_PRIME);
        Ok(())
    }

    /// 封批：未满拒、重复封拒。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(DC_ESEALED);
        }
        if !self.full() {
            return Err(DC_ESHORT);
        }
        self.sealed = true;
        Ok(())
    }

    /// 条号 → 全域条号（(批-1)×20 + 槽）。
    pub fn global_id(&self, slot: u16) -> Option<u32> {
        if !BatchTally::slot_legal(slot) {
            return None;
        }
        Some(
            (self.no as u32)
                .wrapping_sub(1)
                .wrapping_mul(PER_BATCH as u32)
                .wrapping_add(slot as u32),
        )
    }

    fn fold(&self) -> u64 {
        let mut h = self.digest;
        h = (h ^ self.no as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.filled as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ if self.sealed { 1 } else { 0 }).wrapping_mul(FNV_PRIME);
        h
    }
}

/// 满条断言书：四十批断言账 + 单值 + 冻结位。
#[derive(Clone, Debug)]
pub struct CriteriaBook {
    batches: Vec<BatchTally>,
    single: Option<u64>,
    frozen: bool,
    pub enrolls: u64,
    pub rejects: u64,
}

impl CriteriaBook {
    /// 空书（零批）。
    pub fn empty() -> CriteriaBook {
        CriteriaBook { batches: Vec::new(), single: None, frozen: false, enrolls: 0, rejects: 0 }
    }

    /// 满册开账：批号 1..=40 全部开出，条数为零。
    pub fn full_frame() -> CriteriaBook {
        let mut batches = Vec::with_capacity(BATCHES);
        let mut no = 1u16;
        while (no as usize) <= BATCHES {
            batches.push(BatchTally::new(no));
            no = no.wrapping_add(1);
        }
        CriteriaBook { batches, single: None, frozen: false, enrolls: 0, rejects: 0 }
    }

    pub fn batch_count(&self) -> usize {
        self.batches.len()
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 批引用（批号 1 起，非零基）。
    pub fn batch(&self, no: u16) -> Result<&BatchTally, u32> {
        if no == 0 {
            return Err(DC_EBATCH);
        }
        match self.batches.get((no as usize).wrapping_sub(1)) {
            Some(b) => Ok(b),
            None => Err(DC_EBATCH),
        }
    }

    fn batch_mut(&mut self, no: u16) -> Result<&mut BatchTally, u32> {
        if self.frozen {
            return Err(DC_EFROZEN);
        }
        if no == 0 {
            return Err(DC_EBATCH);
        }
        match self.batches.get_mut((no as usize).wrapping_sub(1)) {
            Some(b) => Ok(b),
            None => Err(DC_EBATCH),
        }
    }

    /// 入册：批号/条号/条序三关，失败计拒。
    pub fn enroll(&mut self, no: u16, slot: u16, tag: u32) -> Result<(), u32> {
        let r = match self.batch_mut(no) {
            Ok(b) => b.enroll(slot, tag),
            Err(e) => Err(e),
        };
        match r {
            Ok(()) => {
                self.enrolls = self.enrolls.wrapping_add(1);
                Ok(())
            }
            Err(e) => {
                self.rejects = self.rejects.wrapping_add(1);
                Err(e)
            }
        }
    }

    /// 封批：满二十条才可封。
    pub fn seal(&mut self, no: u16) -> Result<(), u32> {
        let r = match self.batch_mut(no) {
            Ok(b) => b.seal(),
            Err(e) => Err(e),
        };
        if r.is_err() {
            self.rejects = self.rejects.wrapping_add(1);
        }
        r
    }

    /// 已封批数（只探测不改账）。
    pub fn sealed_batches(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.batches.len() {
            if let Some(b) = self.batches.get(k) {
                if b.sealed {
                    n = n.wrapping_add(1);
                }
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 已入册总条数。
    pub fn total_filled(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.batches.len() {
            if let Some(b) = self.batches.get(k) {
                n = n.wrapping_add(b.filled as u32);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 条序连续：批号自 1 起逐位排到末批。
    pub fn contiguous(&self) -> bool {
        if self.batches.is_empty() {
            return false;
        }
        let mut k = 0usize;
        while k < self.batches.len() {
            match self.batches.get(k) {
                Some(b) => {
                    if b.no as usize != k.wrapping_add(1) {
                        return false;
                    }
                }
                None => return false,
            }
            k = k.wrapping_add(1);
        }
        true
    }

    /// 未封批的批号（诊断用）。
    pub fn open_batches(&self) -> Vec<u16> {
        let mut v: Vec<u16> = Vec::new();
        let mut k = 0usize;
        while k < self.batches.len() {
            if let Some(b) = self.batches.get(k) {
                if !b.sealed {
                    v.push(b.no);
                }
            }
            k = k.wrapping_add(1);
        }
        v
    }

    /// 求满条单值：四十批全封、每批恰二十、总条恰 800、批序连续、尚未出值。
    pub fn contend_single(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(DC_EFROZEN);
        }
        if self.single.is_some() {
            return Err(DC_EDUP);
        }
        if self.batches.len() != BATCHES
            || self.sealed_batches() != BATCHES
            || self.total_filled() != TOTAL
            || !self.contiguous()
        {
            return Err(DC_EUNFULL);
        }
        let mut k = 0usize;
        while k < self.batches.len() {
            match self.batches.get(k) {
                Some(b) => {
                    if !b.sealed || !b.full() {
                        return Err(DC_EUNFULL);
                    }
                }
                None => return Err(DC_EUNFULL),
            }
            k = k.wrapping_add(1);
        }
        let v = self.fold();
        self.single = Some(v);
        Ok(v)
    }

    pub fn single_value(&self) -> Option<u64> {
        self.single
    }

    /// 账面指纹：逐批链式 fold，再混入单值。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut k = 0usize;
        while k < self.batches.len() {
            if let Some(b) = self.batches.get(k) {
                h = (h ^ b.fold()).wrapping_mul(FNV_PRIME);
            }
            k = k.wrapping_add(1);
        }
        let s = match self.single {
            Some(v) => v,
            None => 0,
        };
        h = (h ^ s).wrapping_mul(FNV_PRIME);
        h
    }

    /// 版本冻结：幂等——已冻结直接取回锁定值，绝不按当下账面重算。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.single;
        }
        if self.single.is_none() {
            self.single = Some(self.fold());
        }
        self.frozen = true;
        self.single
    }
}

/// 标准入册序：批 1..=40、条 1..=20，标签由批条坐标派生（确定性）。
pub fn enroll_all(b: &mut CriteriaBook) -> u32 {
    let mut done = 0u32;
    let mut no = 1u16;
    while (no as usize) <= BATCHES {
        let mut slot = 1u16;
        while slot <= PER_BATCH {
            let tag = (no as u32).wrapping_mul(101).wrapping_add(slot as u32);
            if b.enroll(no, slot, tag).is_ok() {
                done = done.wrapping_add(1);
            }
            slot = slot.wrapping_add(1);
        }
        no = no.wrapping_add(1);
    }
    done
}

/// 全批封账，返回封成功批数。
pub fn seal_all(b: &mut CriteriaBook) -> usize {
    let mut n = 0usize;
    let mut no = 1u16;
    while (no as usize) <= BATCHES {
        if b.seal(no).is_ok() {
            n = n.wrapping_add(1);
        }
        no = no.wrapping_add(1);
    }
    n
}

/// 满册装配：开账 → 入册 → 封批 → 求单值。
pub fn build_full() -> (CriteriaBook, Option<u64>) {
    let mut b = CriteriaBook::full_frame();
    let filled = enroll_all(&mut b);
    let sealed = seal_all(&mut b);
    let single = if filled == TOTAL && sealed == BATCHES { b.contend_single().ok() } else { None };
    (b, single)
}

/// 四路注入：条号越界 / 条序跳号 / 未满封批 / 冻结后改写，全部须拒。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];

    // ① 条号越界（0 与 21 双端）
    let mut a = CriteriaBook::full_frame();
    got[0] = a.enroll(1, 0, 1).err() == Some(DC_ESLOT)
        && a.enroll(1, PER_BATCH.wrapping_add(1), 1).err() == Some(DC_ESLOT);

    // ② 条序跳号（跳过 1 号直入 2 号）
    let mut b = CriteriaBook::full_frame();
    got[1] = b.enroll(2, 2, 7).err() == Some(DC_ESEQ);

    // ③ 未满封批
    let mut c = CriteriaBook::full_frame();
    let _ = c.enroll(3, 1, 5);
    got[2] = c.seal(3).err() == Some(DC_ESHORT);

    // ④ 冻结后改写 + 冻结幂等
    let (mut d, single) = build_full();
    let root = d.freeze();
    got[3] = single == root
        && d.freeze() == root
        && d.enroll(1, 1, 9).err() == Some(DC_EFROZEN)
        && d.seal(40).err() == Some(DC_EFROZEN)
        && d.contend_single().err() == Some(DC_EFROZEN);
    got
}

/// 判据台：满条断言单值面的十四条判据。
pub fn run_unx11194_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11194");

    let (mut book, single) = build_full();
    let root = book.freeze();

    // J1 满条入册：八百条一条不缺
    s.add(
        "J1-满条入册",
        book.total_filled() == TOTAL && book.enrolls == TOTAL as u64,
        "四十批各二十条、总条恰为八百",
    );

    // J2 四十批全封
    s.add(
        "J2-四十批全封",
        book.sealed_batches() == BATCHES && book.open_batches().is_empty(),
        "批 1..40 逐批封齐、未封清单为空",
    );

    // J3 满条单值成立
    s.add(
        "J3-满条单值",
        single.is_some() && single == root,
        "800/800 汇出的单值即冻结值",
    );

    // J4 批序连续
    s.add(
        "J4-批序连续",
        book.contiguous() && book.batch_count() == BATCHES,
        "批号 1..40 连续无断档",
    );

    // J5 冻结幂等
    let again = book.freeze();
    s.add(
        "J5-冻结幂等",
        again == root && book.is_frozen(),
        "重复冻结取回同一锁定值",
    );

    // J6 冻结拒改
    s.add(
        "J6-冻结拒改",
        book.enroll(1, 1, 3).err() == Some(DC_EFROZEN)
            && book.seal(1).err() == Some(DC_EFROZEN)
            && book.contend_single().err() == Some(DC_EFROZEN),
        "冻结后入册/封批/求和三路皆拒",
    );

    // J7 未满拒求值
    let mut fresh = CriteriaBook::full_frame();
    let _ = enroll_all(&mut fresh);
    s.add(
        "J7-未封拒求值",
        fresh.contend_single().err() == Some(DC_EUNFULL) && fresh.single_value().is_none(),
        "入册齐但未封批不得出单值",
    );

    // J8 重复求值拒
    let mut dup = CriteriaBook::full_frame();
    let _ = enroll_all(&mut dup);
    let _ = seal_all(&mut dup);
    let first = dup.contend_single();
    s.add(
        "J8-重复求值拒",
        first.is_ok() && dup.contend_single().err() == Some(DC_EDUP),
        "单值只出一次、二次求值必拒",
    );

    // J9 条号越界双端
    let mut sl = CriteriaBook::full_frame();
    s.add(
        "J9-条号越界",
        sl.enroll(1, 0, 1).err() == Some(DC_ESLOT)
            && sl.enroll(1, 21, 1).err() == Some(DC_ESLOT),
        "条号 0 与 21 双端越界皆拒",
    );

    // J10 条序跳号拒
    let mut sq = CriteriaBook::full_frame();
    s.add(
        "J10-跳号拒",
        sq.enroll(5, 3, 1).err() == Some(DC_ESEQ) && sq.enroll(5, 1, 1).is_ok(),
        "跳号拒、回填首条可入",
    );

    // J11 未满拒封与重复封拒
    let mut sh = CriteriaBook::full_frame();
    let _ = sh.enroll(7, 1, 1);
    let mut full_one = CriteriaBook::full_frame();
    let mut slot = 1u16;
    while slot <= PER_BATCH {
        let _ = full_one.enroll(8, slot, slot as u32);
        slot = slot.wrapping_add(1);
    }
    let first_seal = full_one.seal(8);
    s.add(
        "J11-未满与重封",
        sh.seal(7).err() == Some(DC_ESHORT)
            && first_seal.is_ok()
            && full_one.seal(8).err() == Some(DC_ESEALED),
        "未满拒封、重复封批拒",
    );

    // J12 批号越界
    let b = CriteriaBook::full_frame();
    s.add(
        "J12-批号越界",
        b.batch(0).err() == Some(DC_EBATCH)
            && b.batch(41).err() == Some(DC_EBATCH),
        "批号 0 与 41 越界读写皆拒",
    );

    // J13 未封清单枚举
    let mut part = CriteriaBook::full_frame();
    let _ = enroll_all(&mut part);
    let mut k = 1u16;
    while k < 4 {
        let _ = part.seal(k);
        k = k.wrapping_add(1);
    }
    s.add(
        "J13-未封可枚举",
        part.open_batches().len() == BATCHES - 3 && part.sealed_batches() == 3,
        "未封批号逐一批出、已封计数对平",
    );

    // J14 注入四拒
    let hits = inject_four();
    let mut all = true;
    let mut i = 0usize;
    while i < hits.len() {
        if !hits[i] {
            all = false;
        }
        i = i.wrapping_add(1);
    }
    s.add("J14-注入四拒", all, "越界/跳号/未满/冻结四路全拒");

    // J15 动态明细：单值、入册数与指纹
    s.add_dyn(
        "J15-满册明细",
        book.total_filled() == TOTAL && root.is_some(),
        &format!(
            "filled={} sealed={} single={:#x}",
            book.total_filled(),
            book.sealed_batches(),
            root.unwrap_or(0)
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11194_criteria_flow() {
        let (mut b, single) = build_full();
        assert_eq!(b.total_filled(), TOTAL);
        assert_eq!(b.sealed_batches(), BATCHES);
        assert!(b.contiguous());
        assert!(single.is_some());

        let root = b.freeze();
        assert_eq!(root, single);
        assert_eq!(b.freeze(), root, "冻结幂等");
        assert_eq!(b.enroll(1, 1, 1).err(), Some(DC_EFROZEN));

        let mut e = CriteriaBook::empty();
        assert_eq!(e.contend_single().err(), Some(DC_EUNFULL));
        assert!(e.freeze().is_some());
        assert_eq!(e.freeze(), e.single_value());

        assert!(inject_four().iter().all(|h| *h));
    }

    #[test]
    fn unx11194_checks_green() {
        let s = run_unx11194_checks();
        assert!(s.all_passed(), "UNX-F11194 判据未全绿");
    }
}
