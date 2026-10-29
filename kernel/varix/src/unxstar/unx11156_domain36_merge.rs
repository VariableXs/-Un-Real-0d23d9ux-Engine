//! UNX-F11156 域 36 批清账合账（B01–B36 全域清账总账）（0x1f380 段）
//!
//! 纯功能模型：清账两段合为一本域总账——B01–B20 为段一（20 批）、B21–B36 为
//! 段二（16 批），共 36 批、216,000 行、720 判据。合账要求批号 1..36 连续、
//! 段签与批号一致、三数对平；合账闭合并冻结后，全域清账不得再动一笔。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f380;
/// 批号越界（只认 B01–B36）
pub const M3_ENO: u32 = SEG_BASE + 0x01;
/// 批重复合账
pub const M3_EDUP: u32 = SEG_BASE + 0x02;
/// 批序倒挂
pub const M3_EORDER: u32 = SEG_BASE + 0x03;
/// 段签与批号不符，或行数/判据为零（形状违规）
pub const M3_ESEG: u32 = SEG_BASE + 0x04;
/// 三十六批未齐
pub const M3_ESHORT: u32 = SEG_BASE + 0x05;
/// 合账三数不符
pub const M3_ESUM: u32 = SEG_BASE + 0x06;
/// 段界处缺口
pub const M3_EGAP: u32 = SEG_BASE + 0x07;
/// 终值已锁定
pub const M3_EFROZEN: u32 = SEG_BASE + 0x08;

/// 域批数。
pub const BATCHES: usize = 36;
/// 段一批数。
pub const SEG1_BATCHES: usize = 20;
/// 段二批数。
pub const SEG2_BATCHES: usize = 16;
/// 每批行数。
pub const LINES_PER_BATCH: u32 = 6_000;
/// 每批判据。
pub const CRITERIA_PER_BATCH: u16 = 20;
/// 域行数。
pub const DOMAIN_LINES: u64 = 216_000;
/// 域判据。
pub const DOMAIN_CRITERIA: u64 = 720;
/// 段一签。
pub const TAG_SEG1: u8 = 1;
/// 段二签。
pub const TAG_SEG2: u8 = 2;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一批合账件。
#[derive(Clone, Copy, Debug)]
pub struct MergeBatch {
    pub batch: u8,
    pub tag: u8,
    pub lines: u32,
    pub criteria: u16,
}

impl MergeBatch {
    /// 段签是否与批号相符（B01–B20 段一，B21–B36 段二）。
    pub fn tag_matches(&self) -> bool {
        let want = if (self.batch as usize) <= SEG1_BATCHES {
            TAG_SEG1
        } else {
            TAG_SEG2
        };
        self.tag == want
    }
}

/// 域三十六批清账合账。
#[derive(Clone, Debug)]
pub struct MergeLedger {
    batches: Vec<MergeBatch>,
    settled: Option<u64>,
    frozen: bool,
    pub accepts: u64,
    pub rejects: u64,
}

impl MergeLedger {
    /// 空账。
    pub fn new() -> MergeLedger {
        MergeLedger {
            batches: Vec::new(),
            settled: None,
            frozen: false,
            accepts: 0,
            rejects: 0,
        }
    }

    /// 已合批数。
    pub fn len(&self) -> usize {
        self.batches.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.batches.is_empty()
    }

    /// 取某批。
    pub fn batch(&self, no: usize) -> Option<MergeBatch> {
        let mut i = 0usize;
        while i < self.batches.len() {
            if self.batches[i].batch as usize == no {
                return Some(self.batches[i]);
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 合一批：冻结 → 越界 → 重记 → 倒序 → 零值 → 段签。
    ///
    /// 重记先于倒序；零值先于段签，形状两路各自可达。
    pub fn merge(&mut self, no: usize, tag: u8, lines: u32, criteria: u16) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(M3_EFROZEN);
        }
        if no == 0 || no > BATCHES {
            return self.fail(M3_ENO);
        }
        if self.batch(no).is_some() {
            return self.fail(M3_EDUP);
        }
        if let Some(last) = self.batches.last() {
            if no <= last.batch as usize {
                return self.fail(M3_EORDER);
            }
        }
        if lines == 0 || criteria == 0 {
            return self.fail(M3_ESEG);
        }
        let entry = MergeBatch {
            batch: no as u8,
            tag,
            lines,
            criteria,
        };
        if !entry.tag_matches() {
            return self.fail(M3_ESEG);
        }
        self.batches.push(entry);
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

    /// 段签计数（段一, 段二）。
    pub fn seg_counts(&self) -> (u32, u32) {
        let mut a = 0u32;
        let mut b = 0u32;
        let mut i = 0usize;
        while i < self.batches.len() {
            if self.batches[i].tag == TAG_SEG1 {
                a = a.wrapping_add(1);
            } else if self.batches[i].tag == TAG_SEG2 {
                b = b.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        (a, b)
    }

    /// 缺批表。
    pub fn gaps(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut n = 1usize;
        while n <= BATCHES {
            if self.batch(n).is_none() {
                out.push(n as u32);
            }
            n = n.wrapping_add(1);
        }
        out
    }

    /// 批号严格连续 1..36。
    pub fn contiguous(&self) -> bool {
        if self.batches.len() != BATCHES || !self.gaps().is_empty() {
            return false;
        }
        let mut i = 0usize;
        while i < self.batches.len() {
            if self.batches[i].batch as usize != i.wrapping_add(1) {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 段界断签：B20 与 B21 只有一侧在账（段一收口而段二未起，或反之）。
    pub fn boundary_gap(&self) -> bool {
        self.batch(SEG1_BATCHES).is_some() != self.batch(SEG1_BATCHES + 1).is_some()
    }

    /// 逐批同形。
    pub fn uniform(&self) -> bool {
        let mut i = 0usize;
        while i < self.batches.len() {
            if self.batches[i].lines != LINES_PER_BATCH
                || self.batches[i].criteria != CRITERIA_PER_BATCH
                || !self.batches[i].tag_matches()
            {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 合账指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.batches.len() {
            let b = self.batches[i];
            h = (h ^ b.batch as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ b.tag as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ b.lines as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ b.criteria as u64).wrapping_mul(FNV_PRIME);
            i = i.wrapping_add(1);
        }
        h = (h ^ self.batches.len() as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.total_lines()).wrapping_mul(FNV_PRIME);
        (h ^ self.total_criteria()).wrapping_mul(FNV_PRIME)
    }

    /// 合账闭合：段界断签 → 未齐 → 三数 → 连续同形。
    pub fn settle(&mut self) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(M3_EFROZEN);
        }
        if self.boundary_gap() {
            return self.fail(M3_EGAP);
        }
        if self.batches.len() != BATCHES {
            return self.fail(M3_ESHORT);
        }
        if self.total_lines() != DOMAIN_LINES || self.total_criteria() != DOMAIN_CRITERIA {
            return self.fail(M3_ESUM);
        }
        let (a, b) = self.seg_counts();
        if a != SEG1_BATCHES as u32 || b != SEG2_BATCHES as u32 {
            return self.fail(M3_ESEG);
        }
        if !self.contiguous() || !self.uniform() {
            return self.fail(M3_ESUM);
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

/// 建账：B01–B36 逐批段签对号，各 6,000 行、20 判据，再合账。
pub fn build_domain36() -> (MergeLedger, bool) {
    let mut l = MergeLedger::new();
    let mut n = 1usize;
    while n <= BATCHES {
        let tag = if n <= SEG1_BATCHES { TAG_SEG1 } else { TAG_SEG2 };
        if l.merge(n, tag, LINES_PER_BATCH, CRITERIA_PER_BATCH).is_err() {
            return (l, false);
        }
        n = n.wrapping_add(1);
    }
    let ok = l.settle().is_ok();
    (l, ok)
}

/// 四类注入：批号越界 / 重记与倒序 / 段签错配与零值 / 终账后再合。
pub fn inject_four() -> [bool; 4] {
    // ① 批号越界
    let mut a0 = MergeLedger::new();
    let r0 = a0.merge(0, TAG_SEG1, 1, 1).err() == Some(M3_ENO)
        && a0.merge(BATCHES + 1, TAG_SEG2, 1, 1).err() == Some(M3_ENO)
        && a0.is_empty();

    // ② 重记与倒序
    let mut a1 = MergeLedger::new();
    let s0 = a1.merge(1, TAG_SEG1, LINES_PER_BATCH, 20).is_ok();
    let s1 = a1.merge(2, TAG_SEG1, LINES_PER_BATCH, 20).is_ok();
    let dup = s0 && s1 && a1.merge(1, TAG_SEG1, LINES_PER_BATCH, 20).err() == Some(M3_EDUP);
    let mut a2 = MergeLedger::new();
    let _ = a2.merge(3, TAG_SEG1, LINES_PER_BATCH, 20);
    let order = a2.merge(2, TAG_SEG1, LINES_PER_BATCH, 20).err() == Some(M3_EORDER);
    let r1 = dup && order;

    // ③ 段签错配与零值
    let mut a3 = MergeLedger::new();
    let seg = a3.merge(5, TAG_SEG2, LINES_PER_BATCH, 20).err() == Some(M3_ESEG)
        && a3.merge(7, TAG_SEG1, 0, 20).err() == Some(M3_ESEG)
        && a3.is_empty();
    let mut a4 = MergeLedger::new();
    let mut n = 1usize;
    while n <= BATCHES {
        let tag = if n <= SEG1_BATCHES { TAG_SEG1 } else { TAG_SEG2 };
        let _ = a4.merge(n, tag, LINES_PER_BATCH, 20);
        n = n.wrapping_add(1);
    }
    let r2 = seg && a4.settle().is_ok();

    // ④ 终账后再合 / 再清
    let (mut a5, ok5) = build_domain36();
    let locked = a5.freeze().is_some();
    let r3 = ok5
        && locked
        && a5.merge(1, TAG_SEG1, LINES_PER_BATCH, 20).err() == Some(M3_EFROZEN)
        && a5.settle().err() == Some(M3_EFROZEN);

    [r0, r1, r2, r3]
}

/// 判据：UNX-F11156 域 36 批清账合账。
pub fn run_unx11156_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11156");
    let (mut l, ok) = build_domain36();

    // X1 三十六批齐备
    s.add_dyn(
        "X1-三十六批齐备",
        ok && l.len() == BATCHES && l.gaps().is_empty(),
        &format!("batches={}/{} gaps={}", l.len(), BATCHES, l.gaps().len()),
    );

    // X2 批号连续
    s.add("X2-批号连续", l.contiguous(), "B01–B36 连续无缺口");

    // X3 段签计数
    let (a, b) = l.seg_counts();
    s.add_dyn(
        "X3-段签计数对平",
        a == SEG1_BATCHES as u32 && b == SEG2_BATCHES as u32,
        &format!("seg1={} seg2={}", a, b),
    );

    // X4 段界不断签
    s.add("X4-段界不断签", !l.boundary_gap(), "B20 与 B21 两侧同时在账");

    // X5 行数对平
    s.add_dyn(
        "X5-行数对平",
        l.total_lines() == DOMAIN_LINES,
        &format!("lines={}/{}", l.total_lines(), DOMAIN_LINES),
    );

    // X6 判据对平
    s.add_dyn(
        "X6-判据对平",
        l.total_criteria() == DOMAIN_CRITERIA,
        &format!("criteria={}/{}", l.total_criteria(), DOMAIN_CRITERIA),
    );

    // X7 逐批同形且段签对号
    s.add("X7-逐批同形", l.uniform(), "每批 6,000 行、20 判据且段签与批号相符");

    // X8 终值即指纹
    let root = l.settled_value();
    s.add("X8-终值即指纹", root == Some(l.fold()), "合账单值等于三十六批指纹");

    // X9 冻结幂等
    let f1 = l.freeze();
    let f2 = l.freeze();
    s.add("X9-冻结幂等", f1.is_some() && f1 == f2, "二次冻结返回同一单值");

    // X10 冻结后拒合拒清
    s.add(
        "X10-冻结后拒合拒清",
        l.is_frozen()
            && l.merge(1, TAG_SEG1, LINES_PER_BATCH, 20).err() == Some(M3_EFROZEN)
            && l.settle().err() == Some(M3_EFROZEN),
        "冻结后 merge/settle 都落 M3_EFROZEN",
    );

    // X11 越界 / 重记 / 倒序 / 段签 / 零值
    let mut g = MergeLedger::new();
    let g0 = g.merge(1, TAG_SEG1, LINES_PER_BATCH, 20).is_ok();
    let mut o = MergeLedger::new();
    let _ = o.merge(3, TAG_SEG1, LINES_PER_BATCH, 20);
    s.add(
        "X11-五路拒止",
        g.merge(0, TAG_SEG1, 1, 1).err() == Some(M3_ENO)
            && g.merge(BATCHES + 1, TAG_SEG2, 1, 1).err() == Some(M3_ENO)
            && g0
            && g.merge(1, TAG_SEG1, LINES_PER_BATCH, 20).err() == Some(M3_EDUP)
            && o.merge(2, TAG_SEG1, LINES_PER_BATCH, 20).err() == Some(M3_EORDER)
            && g.merge(21, TAG_SEG1, LINES_PER_BATCH, 20).err() == Some(M3_ESEG)
            && g.merge(22, TAG_SEG2, 0, 20).err() == Some(M3_ESEG),
        "越界/重记/倒序/段签/零值各按其码",
    );

    // X12 段界断签 / 未齐 / 三数不符
    let mut q1 = MergeLedger::new();
    let mut n = 1usize;
    while n <= SEG1_BATCHES {
        let _ = q1.merge(n, TAG_SEG1, LINES_PER_BATCH, 20);
        n = n.wrapping_add(1);
    }
    let mut q2 = MergeLedger::new();
    let mut n = 1usize;
    while n <= 10 {
        let _ = q2.merge(n, TAG_SEG1, LINES_PER_BATCH, 20);
        n = n.wrapping_add(1);
    }
    let mut q3 = MergeLedger::new();
    let mut n = 1usize;
    while n <= BATCHES {
        let tag = if n <= SEG1_BATCHES { TAG_SEG1 } else { TAG_SEG2 };
        let lines = if n == 4 {
            LINES_PER_BATCH.wrapping_sub(1)
        } else {
            LINES_PER_BATCH
        };
        let _ = q3.merge(n, tag, lines, 20);
        n = n.wrapping_add(1);
    }
    let hits = inject_four();
    let all = hits[0] && hits[1] && hits[2] && hits[3];
    let (mut fresh, ok2) = build_domain36();
    let root2 = fresh.freeze().unwrap_or(0);
    s.add_dyn(
        "X12-三拒与注入",
        q1.settle().err() == Some(M3_EGAP)
            && q2.settle().err() == Some(M3_ESHORT)
            && q3.settle().err() == Some(M3_ESUM)
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
    fn f11156_flow() {
        let (mut l, ok) = build_domain36();
        assert!(ok);
        assert_eq!(l.len(), BATCHES);
        assert!(l.contiguous());
        assert!(l.uniform());
        assert!(!l.boundary_gap());
        assert_eq!(l.seg_counts(), (20, 16));
        assert_eq!(l.total_lines(), DOMAIN_LINES);
        assert_eq!(l.total_criteria(), DOMAIN_CRITERIA);
        let root = l.settled_value();
        assert_eq!(root, Some(l.fold()));
        assert_eq!(l.freeze(), root);
        assert_eq!(l.freeze(), root);
        assert!(l.is_frozen());
        assert_eq!(l.merge(1, TAG_SEG1, LINES_PER_BATCH, 20), Err(M3_EFROZEN));
        assert_eq!(l.settle(), Err(M3_EFROZEN));
    }

    #[test]
    fn f11156_gates() {
        let mut l = MergeLedger::new();
        assert!(l.merge(1, TAG_SEG1, LINES_PER_BATCH, 20).is_ok());
        assert!(l.merge(2, TAG_SEG1, LINES_PER_BATCH, 20).is_ok());
        assert_eq!((l.accepts, l.rejects), (2, 0));
        assert_eq!(l.merge(0, TAG_SEG1, 1, 1), Err(M3_ENO));
        assert_eq!(l.merge(1, TAG_SEG1, LINES_PER_BATCH, 20), Err(M3_EDUP));
        assert_eq!((l.accepts, l.rejects), (2, 2));

        let mut o = MergeLedger::new();
        let _ = o.merge(3, TAG_SEG1, LINES_PER_BATCH, 20);
        assert_eq!(o.merge(2, TAG_SEG1, LINES_PER_BATCH, 20), Err(M3_EORDER));
        assert_eq!((o.accepts, o.rejects), (1, 1));

        let mut sg = MergeLedger::new();
        assert_eq!(sg.merge(5, TAG_SEG2, LINES_PER_BATCH, 20), Err(M3_ESEG));
        assert_eq!(sg.merge(5, TAG_SEG1, 0, 20), Err(M3_ESEG));
        assert_eq!(sg.merge(5, TAG_SEG1, LINES_PER_BATCH, 0), Err(M3_ESEG));
        assert_eq!((sg.accepts, sg.rejects), (0, 3));

        let mut q = MergeLedger::new();
        let mut n = 1usize;
        while n <= SEG1_BATCHES {
            let _ = q.merge(n, TAG_SEG1, LINES_PER_BATCH, 20);
            n = n.wrapping_add(1);
        }
        assert_eq!(q.settle(), Err(M3_EGAP));
        assert_eq!((q.accepts, q.rejects), (20, 1));

        let mut sh = MergeLedger::new();
        let mut n = 1usize;
        while n <= 10 {
            let _ = sh.merge(n, TAG_SEG1, LINES_PER_BATCH, 20);
            n = n.wrapping_add(1);
        }
        assert_eq!(sh.settle(), Err(M3_ESHORT));
        assert_eq!((sh.accepts, sh.rejects), (10, 1));

        let mut m = MergeLedger::new();
        let mut n = 1usize;
        while n <= BATCHES {
            let tag = if n <= SEG1_BATCHES { TAG_SEG1 } else { TAG_SEG2 };
            let lines = if n == 4 {
                LINES_PER_BATCH.wrapping_sub(1)
            } else {
                LINES_PER_BATCH
            };
            let _ = m.merge(n, tag, lines, 20);
            n = n.wrapping_add(1);
        }
        assert_eq!(m.settle(), Err(M3_ESUM));
        assert_eq!((m.accepts, m.rejects), (36, 1));

        let (mut d, ok) = build_domain36();
        assert!(ok);
        assert!(d.freeze().is_some());
        assert_eq!(d.merge(1, TAG_SEG1, LINES_PER_BATCH, 20), Err(M3_EFROZEN));
        assert_eq!(d.settle(), Err(M3_EFROZEN));
        assert_eq!((d.accepts, d.rejects), (36, 2));
    }

    #[test]
    fn f11156_checks_green() {
        assert!(run_unx11156_checks().all_passed());
    }
}
