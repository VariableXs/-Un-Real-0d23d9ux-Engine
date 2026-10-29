//! UNX-F11195 域闭账断言二（240,000/240,000 满账单值）（0x1f600 段）
//!
//! 锚点：UNX-F11195-J1 满账断言单值成立（10/10），断言账版本冻结（10/10）。
//!
//! 纯功能模型：域行数账按四十批各六千行立定额，逐批按增量入账（不得超定额、
//! 不得零增量），批满才可封；四十批全封且总额恰为 240,000 时汇出满账单值。
//! 另设复算面：外部复算表逐批对账，任一批漂移即 EDRIFT 拒止。单值冻结后
//! 入账/封批/求和全拒，重复冻结取回原值。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f600;
/// 批号越界（1..=40）
pub const LN_EBATCH: u32 = SEG_BASE + 0x01;
/// 零增量入账
pub const LN_EZERO: u32 = SEG_BASE + 0x02;
/// 入账超出批定额
pub const LN_EOVER: u32 = SEG_BASE + 0x03;
/// 未满即封批
pub const LN_ESHORT: u32 = SEG_BASE + 0x04;
/// 冻结后改写
pub const LN_EFROZEN: u32 = SEG_BASE + 0x05;
/// 复算表长度不符
pub const LN_ESUM: u32 = SEG_BASE + 0x06;
/// 重复求单值
pub const LN_EDUP: u32 = SEG_BASE + 0x07;
/// 复算漂移
pub const LN_EDRIFT: u32 = SEG_BASE + 0x08;

/// 批数（B01–B40）。
pub const BATCHES: usize = 40;
/// 每批定额（240,000 / 40 = 6,000）。
pub const QUOTA_EACH: u32 = 6000;
/// 满账总额。
pub const TOTAL_LINES: u32 = 240_000;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 单批行数账：定额、已入账、封批位。
#[derive(Clone, Debug, PartialEq)]
pub struct LineBatch {
    pub no: u16,
    pub quota: u32,
    pub counted: u32,
    pub sealed: bool,
}

impl LineBatch {
    fn new(no: u16, quota: u32) -> LineBatch {
        LineBatch { no, quota, counted: 0, sealed: false }
    }

    /// 剩余可入账额度。
    pub fn remaining(&self) -> u32 {
        self.quota.saturating_sub(self.counted)
    }

    /// 是否恰满定额。
    pub fn full(&self) -> bool {
        self.counted == self.quota
    }

    /// 增量入账：封批拒、零增量拒、超定额拒。
    pub fn account(&mut self, delta: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(LN_EFROZEN);
        }
        if delta == 0 {
            return Err(LN_EZERO);
        }
        if delta > self.remaining() {
            return Err(LN_EOVER);
        }
        self.counted = self.counted.wrapping_add(delta);
        Ok(())
    }

    /// 封批：定额须恰满。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(LN_EFROZEN);
        }
        if !self.full() {
            return Err(LN_ESHORT);
        }
        self.sealed = true;
        Ok(())
    }

    /// 单批指纹：批号 + 定额 + 已入账 + 封位。
    fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = (h ^ self.no as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.quota as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.counted as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ if self.sealed { 1 } else { 0 }).wrapping_mul(FNV_PRIME);
        h
    }
}

/// 满账断言书：四十批行数账 + 单值 + 冻结位。
#[derive(Clone, Debug)]
pub struct LineBook {
    batches: Vec<LineBatch>,
    single: Option<u64>,
    frozen: bool,
    pub chunks: u64,
    pub rejects: u64,
}

impl LineBook {
    /// 空书（零批）。
    pub fn empty() -> LineBook {
        LineBook { batches: Vec::new(), single: None, frozen: false, chunks: 0, rejects: 0 }
    }

    /// 定额开账：四十批各六千行。
    pub fn frame() -> LineBook {
        let mut batches = Vec::with_capacity(BATCHES);
        let mut no = 1u16;
        while (no as usize) <= BATCHES {
            batches.push(LineBatch::new(no, QUOTA_EACH));
            no = no.wrapping_add(1);
        }
        LineBook { batches, single: None, frozen: false, chunks: 0, rejects: 0 }
    }

    pub fn batch_count(&self) -> usize {
        self.batches.len()
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 批引用（批号 1 起）。
    pub fn batch(&self, no: u16) -> Result<&LineBatch, u32> {
        if no == 0 {
            return Err(LN_EBATCH);
        }
        match self.batches.get((no as usize).wrapping_sub(1)) {
            Some(b) => Ok(b),
            None => Err(LN_EBATCH),
        }
    }

    fn batch_mut(&mut self, no: u16) -> Result<&mut LineBatch, u32> {
        if self.frozen {
            return Err(LN_EFROZEN);
        }
        if no == 0 {
            return Err(LN_EBATCH);
        }
        match self.batches.get_mut((no as usize).wrapping_sub(1)) {
            Some(b) => Ok(b),
            None => Err(LN_EBATCH),
        }
    }

    /// 定额总和（须恒等于 240,000）。
    pub fn total_quota(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.batches.len() {
            if let Some(b) = self.batches.get(k) {
                n = n.wrapping_add(b.quota);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 已入账总和。
    pub fn total_counted(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.batches.len() {
            if let Some(b) = self.batches.get(k) {
                n = n.wrapping_add(b.counted);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 已封批数。
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

    /// 增量入账：批级三关，失败计拒。
    pub fn account(&mut self, no: u16, delta: u32) -> Result<(), u32> {
        let r = match self.batch_mut(no) {
            Ok(b) => b.account(delta),
            Err(e) => Err(e),
        };
        match r {
            Ok(()) => {
                self.chunks = self.chunks.wrapping_add(1);
                Ok(())
            }
            Err(e) => {
                self.rejects = self.rejects.wrapping_add(1);
                Err(e)
            }
        }
    }

    /// 封批。
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

    /// 复算面：外部逐批复算表与账面逐批对平，任一批漂移即拒。
    pub fn verify(&self, totals: &[u32]) -> Result<u64, u32> {
        if totals.len() != self.batches.len() {
            return Err(LN_ESUM);
        }
        let mut h = FNV_OFF;
        let mut k = 0usize;
        while k < self.batches.len() {
            let row = match totals.get(k) {
                Some(v) => *v,
                None => return Err(LN_ESUM),
            };
            let b = match self.batches.get(k) {
                Some(b) => b,
                None => return Err(LN_ESUM),
            };
            if row != b.counted {
                return Err(LN_EDRIFT);
            }
            h = (h ^ row as u64).wrapping_mul(FNV_PRIME);
            k = k.wrapping_add(1);
        }
        Ok(h)
    }

    /// 未封批号（诊断用）。
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

    /// 求满账单值：四十批全封、总额恰 240,000、定额总和守恒、尚未出值。
    pub fn contend_single(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(LN_EFROZEN);
        }
        if self.single.is_some() {
            return Err(LN_EDUP);
        }
        if self.batches.len() != BATCHES
            || self.total_quota() != TOTAL_LINES
            || self.total_counted() != TOTAL_LINES
            || self.sealed_batches() != BATCHES
        {
            return Err(LN_ESUM);
        }
        let mut k = 0usize;
        while k < self.batches.len() {
            match self.batches.get(k) {
                Some(b) => {
                    if !b.sealed || !b.full() {
                        return Err(LN_ESUM);
                    }
                }
                None => return Err(LN_ESUM),
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

    /// 账面指纹：逐批链式 fold 再混入单值。
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

    /// 版本冻结：幂等取回锁定值。
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

/// 标准入账序：每批分四笔各 1,500 行入账，再逐批封。
pub fn account_all(b: &mut LineBook) -> u32 {
    let mut done = 0u32;
    let mut no = 1u16;
    while (no as usize) <= BATCHES {
        let mut w = 0u32;
        while w < 4 {
            if b.account(no, QUOTA_EACH / 4).is_ok() {
                done = done.wrapping_add(QUOTA_EACH / 4);
            }
            w = w.wrapping_add(1);
        }
        let _ = b.seal(no);
        no = no.wrapping_add(1);
    }
    done
}

/// 满账装配：开账 → 入账 → 封批 → 求单值。
pub fn build_full() -> (LineBook, Option<u64>) {
    let mut b = LineBook::frame();
    let accounted = account_all(&mut b);
    let single = if accounted == TOTAL_LINES { b.contend_single().ok() } else { None };
    (b, single)
}

/// 四路注入：零增量 / 超定额 / 未满封批 / 冻结后改写，全部须拒。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];

    // ① 零增量
    let mut a = LineBook::frame();
    got[0] = a.account(1, 0).err() == Some(LN_EZERO);

    // ② 超定额（定额 +1）
    let mut b = LineBook::frame();
    got[1] = b.account(2, QUOTA_EACH.wrapping_add(1)).err() == Some(LN_EOVER);

    // ③ 未满封批
    let mut c = LineBook::frame();
    let _ = c.account(3, 1);
    got[2] = c.seal(3).err() == Some(LN_ESHORT);

    // ④ 冻结后改写 + 冻结幂等
    let (mut d, single) = build_full();
    let root = d.freeze();
    got[3] = single == root
        && d.freeze() == root
        && d.account(1, 1).err() == Some(LN_EFROZEN)
        && d.seal(1).err() == Some(LN_EFROZEN)
        && d.contend_single().err() == Some(LN_EFROZEN);
    got
}

/// 判据台：满账断言单值面的十四条判据。
pub fn run_unx11195_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11195");

    let (mut book, single) = build_full();
    let root = book.freeze();

    // J1 定额守恒：四十批定额和恰为 240,000
    s.add(
        "J1-定额守恒",
        book.total_quota() == TOTAL_LINES && book.batch_count() == BATCHES,
        "40 × 6,000 = 240,000 逐批对平",
    );

    // J2 满额入账
    s.add(
        "J2-满额入账",
        book.total_counted() == TOTAL_LINES && book.chunks == (BATCHES as u64) * 4,
        "四十批各四笔入账、总额恰满",
    );

    // J3 四十批全封
    s.add(
        "J3-四十批全封",
        book.sealed_batches() == BATCHES && book.open_batches().is_empty(),
        "批 1..40 全封、未封清单为空",
    );

    // J4 满账单值成立
    s.add(
        "J4-满账单值",
        single.is_some() && single == root,
        "240,000/240,000 汇出的单值即冻结值",
    );

    // J5 冻结幂等
    s.add(
        "J5-冻结幂等",
        book.freeze() == root && book.is_frozen(),
        "重复冻结取回同一锁定值",
    );

    // J6 冻结拒改
    s.add(
        "J6-冻结拒改",
        book.account(1, 1).err() == Some(LN_EFROZEN)
            && book.seal(1).err() == Some(LN_EFROZEN)
            && book.contend_single().err() == Some(LN_EFROZEN),
        "冻结后入账/封批/求和三路皆拒",
    );

    // J7 零增量拒
    let mut z = LineBook::frame();
    s.add("J7-零增量拒", z.account(1, 0).err() == Some(LN_EZERO), "零增量入账必拒");

    // J8 超定额拒且账面不损
    let mut o = LineBook::frame();
    let _ = o.account(2, QUOTA_EACH);
    s.add(
        "J8-超定额拒",
        o.account(2, 1).err() == Some(LN_EOVER)
            && o.batch(2).map(|b| b.counted).ok() == Some(QUOTA_EACH),
        "定额满后再入一笔必拒、账面不损",
    );

    // J9 未满拒封
    let mut sh = LineBook::frame();
    let _ = sh.account(4, QUOTA_EACH - 1);
    s.add(
        "J9-未满拒封",
        sh.seal(4).err() == Some(LN_ESHORT) && sh.sealed_batches() == 0,
        "差一行也不得封批",
    );

    // J10 复算对平
    let mut rows: Vec<u32> = Vec::new();
    let mut k = 0usize;
    while k < BATCHES {
        rows.push(QUOTA_EACH);
        k = k.wrapping_add(1);
    }
    let ok_verify = book.verify(&rows);
    s.add(
        "J10-复算对平",
        ok_verify.is_ok() && ok_verify.ok().is_some(),
        "外部复算表逐批与账面一致",
    );

    // J11 复算漂移拒
    let mut bad = rows.clone();
    if let Some(slot) = bad.get_mut(7) {
        *slot = QUOTA_EACH.wrapping_sub(1);
    }
    s.add(
        "J11-复算漂移",
        book.verify(&bad).err() == Some(LN_EDRIFT),
        "任一批漂移即拒止",
    );

    // J12 复算表长度不符
    let short = &rows[..BATCHES - 1];
    s.add(
        "J12-复算表短",
        book.verify(short).err() == Some(LN_ESUM),
        "复算表缺批即拒、不得静默补零",
    );

    // J13 批号越界与空书
    let mut e = LineBook::empty();
    s.add(
        "J13-越界与空书",
        e.batch(0).err() == Some(LN_EBATCH)
            && e.account(41, 1).err() == Some(LN_EBATCH)
            && e.contend_single().err() == Some(LN_ESUM),
        "批号 0/41 越界、空书不得出单值",
    );

    // J14 重复求值拒
    let mut dup = LineBook::frame();
    let _ = account_all(&mut dup);
    s.add(
        "J14-重复求值拒",
        dup.contend_single().is_ok() && dup.contend_single().err() == Some(LN_EDUP),
        "单值只出一次",
    );

    // J15 注入四拒
    let hits = inject_four();
    let mut all = true;
    let mut i = 0usize;
    while i < hits.len() {
        if !hits[i] {
            all = false;
        }
        i = i.wrapping_add(1);
    }
    s.add("J15-注入四拒", all, "零增量/超定额/未满/冻结四路全拒");

    // J16 动态明细
    s.add_dyn(
        "J16-满账明细",
        book.total_counted() == TOTAL_LINES,
        &format!(
            "counted={} quota={} chunks={} root={:#x}",
            book.total_counted(),
            book.total_quota(),
            book.chunks,
            root.unwrap_or(0)
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11195_lines_flow() {
        let (mut b, single) = build_full();
        assert_eq!(b.total_quota(), TOTAL_LINES);
        assert_eq!(b.total_counted(), TOTAL_LINES);
        assert_eq!(b.sealed_batches(), BATCHES);
        assert!(single.is_some());

        let root = b.freeze();
        assert_eq!(root, single);
        assert_eq!(b.freeze(), root, "冻结幂等");
        assert_eq!(b.account(1, 1).err(), Some(LN_EFROZEN));

        let mut z = LineBook::frame();
        assert_eq!(z.account(1, 0).err(), Some(LN_EZERO));
        assert_eq!(z.account(1, QUOTA_EACH + 1).err(), Some(LN_EOVER));
        assert_eq!(z.seal(1).err(), Some(LN_ESHORT));

        let mut e = LineBook::empty();
        assert_eq!(e.contend_single().err(), Some(LN_ESUM));
        assert!(e.freeze().is_some());
        assert_eq!(e.freeze(), e.single_value());

        assert!(inject_four().iter().all(|h| *h));
    }

    #[test]
    fn unx11195_checks_green() {
        let s = run_unx11195_checks();
        assert!(s.all_passed(), "UNX-F11195 判据未全绿");
    }
}
