//! UNX-F11155 清账段二总账（B21–B36 段账闭合+段一对平）（0x1f370 段）
//!
//! 纯功能模型：清账段二覆盖 B21–B36 共十六批，批批 6,000 行、20 判据。段账闭合
//! 要求十六批连续无缺口、逐批同形、三数（批数/行数/判据数）对平；段一对平要求
//! 段二与段一（B01–B20，120,000 行、400 判据）相加后等于域累计数。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f370;
/// 批号越界（只认 B21–B36）
pub const S2_ENO: u32 = SEG_BASE + 0x01;
/// 批重复落账
pub const S2_EDUP: u32 = SEG_BASE + 0x02;
/// 批序倒挂
pub const S2_EORDER: u32 = SEG_BASE + 0x03;
/// 批内行数或判据为零
pub const S2_EZERO: u32 = SEG_BASE + 0x04;
/// 十六批未齐
pub const S2_ESHORT: u32 = SEG_BASE + 0x05;
/// 段三数不符
pub const S2_ESUM: u32 = SEG_BASE + 0x06;
/// 段内缺口
pub const S2_EGAP: u32 = SEG_BASE + 0x07;
/// 终值已锁定
pub const S2_EFROZEN: u32 = SEG_BASE + 0x08;

/// 段二批数。
pub const SEG2_BATCHES: usize = 16;
/// 段二首批号。
pub const FIRST_BATCH: u32 = 21;
/// 每批行数口径。
pub const LINES_PER_BATCH: u32 = 6_000;
/// 每批判据口径。
pub const CRITERIA_PER_BATCH: u16 = 20;
/// 段一行数（B01–B20）。
pub const SEG1_LINES: u64 = 120_000;
/// 段一判据（B01–B20）。
pub const SEG1_CRITERIA: u64 = 400;
/// 段二行数。
pub const SEG2_LINES: u64 = 96_000;
/// 段二判据。
pub const SEG2_CRITERIA: u64 = 320;
/// 域三十六批行数。
pub const DOMAIN36_LINES: u64 = 216_000;
/// 域三十六批判据。
pub const DOMAIN36_CRITERIA: u64 = 720;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一批段二账。
#[derive(Clone, Copy, Debug)]
pub struct SegBatch {
    pub batch: u8,
    pub lines: u32,
    pub criteria: u16,
}

/// 清账段二总账。
#[derive(Clone, Debug)]
pub struct Seg2Ledger {
    batches: Vec<SegBatch>,
    settled: Option<u64>,
    frozen: bool,
    pub accepts: u64,
    pub rejects: u64,
}

impl Seg2Ledger {
    /// 空账。
    pub fn new() -> Seg2Ledger {
        Seg2Ledger {
            batches: Vec::new(),
            settled: None,
            frozen: false,
            accepts: 0,
            rejects: 0,
        }
    }

    /// 已落批数。
    pub fn len(&self) -> usize {
        self.batches.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.batches.is_empty()
    }

    /// 取某批。
    pub fn batch(&self, no: u32) -> Option<SegBatch> {
        let mut i = 0usize;
        while i < self.batches.len() {
            if self.batches[i].batch as u32 == no {
                return Some(self.batches[i]);
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 落一批：冻结 → 越界 → 重记 → 倒序 → 零值。
    pub fn record(&mut self, no: u32, lines: u32, criteria: u16) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(S2_EFROZEN);
        }
        if no < FIRST_BATCH || no >= FIRST_BATCH.wrapping_add(SEG2_BATCHES as u32) {
            return self.fail(S2_ENO);
        }
        if self.batch(no).is_some() {
            return self.fail(S2_EDUP);
        }
        if let Some(last) = self.batches.last() {
            if no <= last.batch as u32 {
                return self.fail(S2_EORDER);
            }
        }
        if lines == 0 || criteria == 0 {
            return self.fail(S2_EZERO);
        }
        self.batches.push(SegBatch {
            batch: no as u8,
            lines,
            criteria,
        });
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> Result<u64, u32> {
        self.rejects = self.rejects.wrapping_add(1);
        Err(code)
    }

    /// 行数合计。
    pub fn total_lines(&self) -> u64 {
        let mut t = 0u64;
        let mut i = 0usize;
        while i < self.batches.len() {
            t = t.wrapping_add(self.batches[i].lines as u64);
            i = i.wrapping_add(1);
        }
        t
    }

    /// 判据合计。
    pub fn total_criteria(&self) -> u64 {
        let mut t = 0u64;
        let mut i = 0usize;
        while i < self.batches.len() {
            t = t.wrapping_add(self.batches[i].criteria as u64);
            i = i.wrapping_add(1);
        }
        t
    }

    /// 缺批表。
    pub fn gaps(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut b = FIRST_BATCH;
        let end = FIRST_BATCH.wrapping_add(SEG2_BATCHES as u32);
        while b < end {
            if self.batch(b).is_none() {
                out.push(b);
            }
            b = b.wrapping_add(1);
        }
        out
    }

    /// 段中缺口（首末批之间缺失的批号；尾部缺批另计 S2_ESHORT）。
    pub fn mid_gaps(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut lo = 0u32;
        let mut hi = 0u32;
        if let Some(first) = self.batches.first() {
            lo = first.batch as u32;
        }
        if let Some(last) = self.batches.last() {
            hi = last.batch as u32;
        }
        let mut b = lo;
        while b < hi && lo < hi {
            if self.batch(b).is_none() {
                out.push(b);
            }
            b = b.wrapping_add(1);
        }
        out
    }

    /// 段内是否连续（无缺口、批序严格）。
    pub fn contiguous(&self) -> bool {
        if self.batches.len() != SEG2_BATCHES || !self.gaps().is_empty() {
            return false;
        }
        let mut i = 0usize;
        while i < self.batches.len() {
            let want = FIRST_BATCH.wrapping_add(i as u32);
            if self.batches[i].batch as u32 != want {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 逐批同形。
    pub fn uniform(&self) -> bool {
        let mut i = 0usize;
        while i < self.batches.len() {
            if self.batches[i].lines != LINES_PER_BATCH
                || self.batches[i].criteria != CRITERIA_PER_BATCH
            {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 与段一对平：段二 + 段一 = 域三十六批。
    pub fn pairs_with_segment1(&self) -> bool {
        self.total_lines().wrapping_add(SEG1_LINES) == DOMAIN36_LINES
            && self.total_criteria().wrapping_add(SEG1_CRITERIA) == DOMAIN36_CRITERIA
    }

    /// 段账指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.batches.len() {
            let b = self.batches[i];
            h = (h ^ b.batch as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ b.lines as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ b.criteria as u64).wrapping_mul(FNV_PRIME);
            i = i.wrapping_add(1);
        }
        h = (h ^ self.batches.len() as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.total_lines()).wrapping_mul(FNV_PRIME);
        (h ^ self.total_criteria()).wrapping_mul(FNV_PRIME)
    }

    /// 段账闭合：十六批齐 → 连续 → 三数对平 → 同形 → 与段一对平。
    pub fn settle(&mut self) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(S2_EFROZEN);
        }
        if !self.mid_gaps().is_empty() {
            return self.fail(S2_EGAP);
        }
        if self.batches.len() != SEG2_BATCHES {
            return self.fail(S2_ESHORT);
        }
        if self.total_lines() != SEG2_LINES || self.total_criteria() != SEG2_CRITERIA {
            return self.fail(S2_ESUM);
        }
        if !self.uniform() || !self.contiguous() {
            return self.fail(S2_ESUM);
        }
        if !self.pairs_with_segment1() {
            return self.fail(S2_ESUM);
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

/// 建账：B21–B36 各 6,000 行、20 判据，再段账闭合。
pub fn build_seg2() -> (Seg2Ledger, bool) {
    let mut l = Seg2Ledger::new();
    let mut i = 0usize;
    while i < SEG2_BATCHES {
        let no = FIRST_BATCH.wrapping_add(i as u32);
        if l.record(no, LINES_PER_BATCH, CRITERIA_PER_BATCH).is_err() {
            return (l, false);
        }
        i = i.wrapping_add(1);
    }
    let ok = l.settle().is_ok();
    (l, ok)
}

/// 四类注入：批号越界 / 重记与倒序 / 零值批 / 终账后再落。
pub fn inject_four() -> [bool; 4] {
    // ① 批号越界
    let mut a0 = Seg2Ledger::new();
    let r0 = a0
        .record(FIRST_BATCH.wrapping_sub(1), 1, 1)
        .err()
        == Some(S2_ENO)
        && a0
            .record(FIRST_BATCH.wrapping_add(SEG2_BATCHES as u32), 1, 1)
            .err()
            == Some(S2_ENO)
        && a0.is_empty();

    // ② 重记与倒序
    let mut a1 = Seg2Ledger::new();
    let s0 = a1.record(21, LINES_PER_BATCH, 20).is_ok();
    let s1 = a1.record(22, LINES_PER_BATCH, 20).is_ok();
    let dup = s0 && s1 && a1.record(21, LINES_PER_BATCH, 20).err() == Some(S2_EDUP);
    let mut a2 = Seg2Ledger::new();
    let _ = a2.record(23, LINES_PER_BATCH, 20);
    let order = a2.record(22, LINES_PER_BATCH, 20).err() == Some(S2_EORDER);
    let r1 = dup && order;

    // ③ 零值与段中缺口
    let mut a3 = Seg2Ledger::new();
    let zero = a3.record(21, 0, 20).err() == Some(S2_EZERO)
        && a3.record(21, LINES_PER_BATCH, 0).err() == Some(S2_EZERO);
    let mut a4 = Seg2Ledger::new();
    let mut i = 0usize;
    while i < SEG2_BATCHES - 1 {
        let no = if i < 9 {
            FIRST_BATCH.wrapping_add(i as u32)
        } else {
            FIRST_BATCH.wrapping_add(i as u32).wrapping_add(1)
        };
        let _ = a4.record(no, LINES_PER_BATCH, CRITERIA_PER_BATCH);
        i = i.wrapping_add(1);
    }
    let gap = a4.settle().err() == Some(S2_EGAP);
    let r2 = zero && gap;

    // ④ 终账后再落 / 再清
    let (mut a5, ok5) = build_seg2();
    let locked = a5.freeze().is_some();
    let r3 = ok5
        && locked
        && a5.record(21, LINES_PER_BATCH, CRITERIA_PER_BATCH)
            .err()
            == Some(S2_EFROZEN)
        && a5.settle().err() == Some(S2_EFROZEN);

    [r0, r1, r2, r3]
}

/// 判据：UNX-F11155 清账段二总账。
pub fn run_unx11155_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11155");
    let (mut l, ok) = build_seg2();

    // X1 十六批齐备
    s.add_dyn(
        "X1-十六批齐备",
        ok && l.len() == SEG2_BATCHES && l.gaps().is_empty(),
        &format!("batches={}/{} gaps={}", l.len(), SEG2_BATCHES, l.gaps().len()),
    );

    // X2 段内连续
    s.add(
        "X2-段内连续",
        l.contiguous() && l.mid_gaps().is_empty(),
        "B21–B36 连续无缺口且批序严格",
    );

    // X3 逐批同形
    s.add("X3-逐批同形", l.uniform(), "十六批各 6,000 行、20 判据");

    // X4 行数对平
    s.add_dyn(
        "X4-行数对平",
        l.total_lines() == SEG2_LINES,
        &format!("lines={}/{}", l.total_lines(), SEG2_LINES),
    );

    // X5 判据对平
    s.add_dyn(
        "X5-判据对平",
        l.total_criteria() == SEG2_CRITERIA,
        &format!("criteria={}/{}", l.total_criteria(), SEG2_CRITERIA),
    );

    // X6 与段一对平
    s.add(
        "X6-与段一对平",
        l.pairs_with_segment1(),
        "段二 96,000 行 + 段一 120,000 行 = 216,000 行",
    );

    // X7 域累计三数
    let domain_lines = l.total_lines().wrapping_add(SEG1_LINES);
    let domain_criteria = l.total_criteria().wrapping_add(SEG1_CRITERIA);
    s.add_dyn(
        "X7-域累计对平",
        domain_lines == DOMAIN36_LINES && domain_criteria == DOMAIN36_CRITERIA,
        &format!(
            "domain={}/{} crit={}/{}",
            domain_lines, DOMAIN36_LINES, domain_criteria, DOMAIN36_CRITERIA
        ),
    );

    // X8 终值即指纹
    let root = l.settled_value();
    s.add("X8-终值即指纹", root == Some(l.fold()), "段账单值等于十六批指纹");

    // X9 冻结幂等
    let f1 = l.freeze();
    let f2 = l.freeze();
    s.add("X9-冻结幂等", f1.is_some() && f1 == f2, "二次冻结返回同一单值");

    // X10 冻结后拒记拒清
    s.add(
        "X10-冻结后拒记拒清",
        l.is_frozen()
            && l.record(21, LINES_PER_BATCH, CRITERIA_PER_BATCH).err() == Some(S2_EFROZEN)
            && l.settle().err() == Some(S2_EFROZEN),
        "冻结后 record/settle 都落 S2_EFROZEN",
    );

    // X11 越界/重记/倒序/零值
    let mut g = Seg2Ledger::new();
    let g0 = g.record(21, LINES_PER_BATCH, CRITERIA_PER_BATCH).is_ok();
    let mut o = Seg2Ledger::new();
    let _ = o.record(23, LINES_PER_BATCH, CRITERIA_PER_BATCH);
    s.add(
        "X11-四路拒止",
        g.record(20, 1, 1).err() == Some(S2_ENO)
            && g.record(37, 1, 1).err() == Some(S2_ENO)
            && g0
            && g.record(21, LINES_PER_BATCH, CRITERIA_PER_BATCH).err() == Some(S2_EDUP)
            && o.record(22, LINES_PER_BATCH, CRITERIA_PER_BATCH).err() == Some(S2_EORDER)
            && g.record(22, 0, 20).err() == Some(S2_EZERO),
        "越界/重记/倒序/零值各按其码",
    );

    // X12 缺口 / 尾缺 / 三数不符
    let mut q1 = Seg2Ledger::new();
    let mut i = 0usize;
    while i < SEG2_BATCHES - 1 {
        let no = if i < 9 {
            FIRST_BATCH.wrapping_add(i as u32)
        } else {
            FIRST_BATCH.wrapping_add(i as u32).wrapping_add(1)
        };
        let _ = q1.record(no, LINES_PER_BATCH, CRITERIA_PER_BATCH);
        i = i.wrapping_add(1);
    }
    let mut q2 = Seg2Ledger::new();
    let mut i = 0usize;
    while i < SEG2_BATCHES - 1 {
        let _ = q2.record(FIRST_BATCH.wrapping_add(i as u32), LINES_PER_BATCH, 20);
        i = i.wrapping_add(1);
    }
    let mut q3 = Seg2Ledger::new();
    let mut i = 0usize;
    while i < SEG2_BATCHES {
        let lines = if i == 4 {
            LINES_PER_BATCH.wrapping_sub(1)
        } else {
            LINES_PER_BATCH
        };
        let _ = q3.record(FIRST_BATCH.wrapping_add(i as u32), lines, 20);
        i = i.wrapping_add(1);
    }
    let hits = inject_four();
    let all = hits[0] && hits[1] && hits[2] && hits[3];
    let (mut fresh, ok2) = build_seg2();
    let root2 = fresh.freeze().unwrap_or(0);
    s.add_dyn(
        "X12-三拒与注入",
        q1.settle().err() == Some(S2_EGAP)
            && q2.settle().err() == Some(S2_ESHORT)
            && q3.settle().err() == Some(S2_ESUM)
            && all
            && ok2
            && fresh.accepts == SEG2_BATCHES as u64
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
    fn f11155_flow() {
        let (mut l, ok) = build_seg2();
        assert!(ok);
        assert_eq!(l.len(), SEG2_BATCHES);
        assert!(l.contiguous());
        assert!(l.uniform());
        assert!(l.mid_gaps().is_empty());
        assert_eq!(l.total_lines(), SEG2_LINES);
        assert_eq!(l.total_criteria(), SEG2_CRITERIA);
        assert!(l.pairs_with_segment1());
        let root = l.settled_value();
        assert_eq!(root, Some(l.fold()));
        assert_eq!(l.freeze(), root);
        assert_eq!(l.freeze(), root);
        assert!(l.is_frozen());
        assert_eq!(l.record(21, LINES_PER_BATCH, CRITERIA_PER_BATCH), Err(S2_EFROZEN));
        assert_eq!(l.settle(), Err(S2_EFROZEN));
    }

    #[test]
    fn f11155_gates() {
        let mut l = Seg2Ledger::new();
        assert!(l.record(21, LINES_PER_BATCH, 20).is_ok());
        assert!(l.record(22, LINES_PER_BATCH, 20).is_ok());
        assert_eq!((l.accepts, l.rejects), (2, 0));
        assert_eq!(l.record(20, 1, 1), Err(S2_ENO));
        assert_eq!(l.record(21, LINES_PER_BATCH, 20), Err(S2_EDUP));
        assert_eq!((l.accepts, l.rejects), (2, 2));

        let mut o = Seg2Ledger::new();
        let _ = o.record(23, LINES_PER_BATCH, 20);
        assert_eq!(o.record(22, LINES_PER_BATCH, 20), Err(S2_EORDER));
        assert_eq!((o.accepts, o.rejects), (1, 1));

        let mut z = Seg2Ledger::new();
        assert_eq!(z.record(21, 0, 20), Err(S2_EZERO));
        assert_eq!(z.record(21, LINES_PER_BATCH, 0), Err(S2_EZERO));
        assert_eq!((z.accepts, z.rejects), (0, 2));

        let mut g = Seg2Ledger::new();
        let mut i = 0usize;
        while i < SEG2_BATCHES - 1 {
            let no = if i < 9 {
                FIRST_BATCH.wrapping_add(i as u32)
            } else {
                FIRST_BATCH.wrapping_add(i as u32).wrapping_add(1)
            };
            let _ = g.record(no, LINES_PER_BATCH, 20);
            i = i.wrapping_add(1);
        }
        assert_eq!(g.mid_gaps(), alloc::vec![30]);
        assert_eq!(g.settle(), Err(S2_EGAP));
        assert_eq!((g.accepts, g.rejects), (15, 1));

        let mut sh = Seg2Ledger::new();
        let mut i = 0usize;
        while i < SEG2_BATCHES - 1 {
            let _ = sh.record(FIRST_BATCH.wrapping_add(i as u32), LINES_PER_BATCH, 20);
            i = i.wrapping_add(1);
        }
        assert_eq!(sh.settle(), Err(S2_ESHORT));
        assert_eq!((sh.accepts, sh.rejects), (15, 1));

        let mut m = Seg2Ledger::new();
        let mut i = 0usize;
        while i < SEG2_BATCHES {
            let lines = if i == 4 {
                LINES_PER_BATCH.wrapping_sub(1)
            } else {
                LINES_PER_BATCH
            };
            let _ = m.record(FIRST_BATCH.wrapping_add(i as u32), lines, 20);
            i = i.wrapping_add(1);
        }
        assert_eq!(m.total_lines(), SEG2_LINES - 1);
        assert_eq!(m.settle(), Err(S2_ESUM));
        assert_eq!((m.accepts, m.rejects), (16, 1));

        let (mut d, ok) = build_seg2();
        assert!(ok);
        assert!(d.freeze().is_some());
        assert_eq!(d.record(21, LINES_PER_BATCH, 20), Err(S2_EFROZEN));
        assert_eq!(d.settle(), Err(S2_EFROZEN));
        assert_eq!((d.accepts, d.rejects), (16, 2));
    }

    #[test]
    fn f11155_checks_green() {
        assert!(run_unx11155_checks().all_passed());
    }
}
