//! UNX-F11160 B38 批收口（清账二段总断言）（0x1f3c0 段）
//!
//! 纯功能模型：B38 收口以「19 判据重放 + 行数守恒」两事为断言。19 条判据逐条
//! 重放须全绿且各带留痕标记；行数守恒要求本批 6,000 行与域累计前值 210,000 行
//! 相加恰为 216,000 行。两条断言同时成立方可收口，收口值一经冻结不再重开。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f3c0;
/// 判据号越界（只认 0..19）
pub const BC_ENO: u32 = SEG_BASE + 0x01;
/// 判据重放重记
pub const BC_EDUP: u32 = SEG_BASE + 0x02;
/// 判据重放倒序
pub const BC_EORDER: u32 = SEG_BASE + 0x03;
/// 留痕标记为零（形状违规）
pub const BC_EMARK: u32 = SEG_BASE + 0x04;
/// 十九条未齐
pub const BC_ESHORT: u32 = SEG_BASE + 0x05;
/// 有判据重放为红
pub const BC_ERED: u32 = SEG_BASE + 0x06;
/// 行数不守恒
pub const BC_ELINE: u32 = SEG_BASE + 0x07;
/// 终值已锁定
pub const BC_EFROZEN: u32 = SEG_BASE + 0x08;

/// B38 判据条数。
pub const CRITERIA: usize = 19;
/// 本批行数。
pub const BATCH_LINES: u64 = 6_000;
/// 域累计前值（B01–B37）。
pub const PRIOR_LINES: u64 = 210_000;
/// 收口后域累计。
pub const DOMAIN_LINES: u64 = 216_000;
/// 每条判据最少留痕标记数。
pub const MIN_MARKS: u32 = 1;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一条收口判据重放件。
#[derive(Clone, Copy, Debug)]
pub struct Criterion {
    pub id: u8,
    pub verdict: bool,
    pub marks: u32,
}

impl Criterion {
    /// 本判据是否留痕到位（真且标记足）。
    pub fn traced(&self) -> bool {
        self.verdict && self.marks >= MIN_MARKS
    }
}

/// B38 批收口账。
#[derive(Clone, Debug)]
pub struct CloseLedger {
    items: Vec<Criterion>,
    lines: u64,
    settled: Option<u64>,
    frozen: bool,
    pub accepts: u64,
    pub rejects: u64,
}

impl CloseLedger {
    /// 空账（本批行数为零，待收口时再记）。
    pub fn new() -> CloseLedger {
        CloseLedger {
            items: Vec::new(),
            lines: 0,
            settled: None,
            frozen: false,
            accepts: 0,
            rejects: 0,
        }
    }

    /// 已重放判据数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 取某条判据。
    pub fn item(&self, id: usize) -> Option<Criterion> {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].id as usize == id {
                return Some(self.items[i]);
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 重放一条判据：冻结 → 越界 → 重记 → 倒序 → 标记为零。
    pub fn replay(&mut self, id: usize, verdict: bool, marks: u32) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(BC_EFROZEN);
        }
        if id >= CRITERIA {
            return self.fail(BC_ENO);
        }
        if self.item(id).is_some() {
            return self.fail(BC_EDUP);
        }
        if let Some(last) = self.items.last() {
            if id <= last.id as usize {
                return self.fail(BC_EORDER);
            }
        }
        if marks < MIN_MARKS {
            return self.fail(BC_EMARK);
        }
        self.items.push(Criterion {
            id: id as u8,
            verdict,
            marks,
        });
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 记本批行数（只允许一次且非零）。
    pub fn set_lines(&mut self, lines: u64) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(BC_EFROZEN);
        }
        if lines == 0 {
            return self.fail(BC_ELINE);
        }
        if self.lines != 0 {
            return self.fail(BC_EDUP);
        }
        self.lines = lines;
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> Result<u64, u32> {
        self.rejects = self.rejects.wrapping_add(1);
        Err(code)
    }

    /// 本批行数。
    pub fn lines(&self) -> u64 {
        self.lines
    }

    /// 行数守恒：前值 + 本批 = 域累计。
    pub fn lines_conserved(&self) -> bool {
        PRIOR_LINES.wrapping_add(self.lines) == DOMAIN_LINES
    }

    /// 留痕标记合计。
    pub fn marks_sum(&self) -> u64 {
        let mut t = 0u64;
        let mut i = 0usize;
        while i < self.items.len() {
            t = t.wrapping_add(self.items[i].marks as u64);
            i = i.wrapping_add(1);
        }
        t
    }

    /// 红判据数。
    pub fn reds(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            if !self.items[i].verdict {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 缺条表。
    pub fn gaps(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut id = 0usize;
        while id < CRITERIA {
            if self.item(id).is_none() {
                out.push(id as u32);
            }
            id = id.wrapping_add(1);
        }
        out
    }

    /// 判据序严格。
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

    /// 逐条留痕到位。
    pub fn all_traced(&self) -> bool {
        let mut i = 0usize;
        while i < self.items.len() {
            if !self.items[i].traced() {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 收口指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.items.len() {
            let it = self.items[i];
            h = (h ^ it.id as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ it.verdict as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ it.marks as u64).wrapping_mul(FNV_PRIME);
            i = i.wrapping_add(1);
        }
        h = (h ^ self.items.len() as u64).wrapping_mul(FNV_PRIME);
        (h ^ self.lines).wrapping_mul(FNV_PRIME)
    }

    /// 收口：十九齐 → 无红 → 留痕齐 → 行数守恒。
    pub fn settle(&mut self) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(BC_EFROZEN);
        }
        if self.items.len() != CRITERIA || !self.gaps().is_empty() {
            return self.fail(BC_ESHORT);
        }
        if self.reds() != 0 || !self.all_traced() {
            return self.fail(BC_ERED);
        }
        if !self.lines_conserved() {
            return self.fail(BC_ELINE);
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

/// 建账：19 判据逐条重放全绿各留 3 痕，本批记 6,000 行，再收口。
pub fn build_b38() -> (CloseLedger, bool) {
    let mut l = CloseLedger::new();
    let mut id = 0usize;
    while id < CRITERIA {
        if l.replay(id, true, 3).is_err() {
            return (l, false);
        }
        id = id.wrapping_add(1);
    }
    if l.set_lines(BATCH_LINES).is_err() {
        return (l, false);
    }
    let ok = l.settle().is_ok();
    (l, ok)
}

/// 四类注入：判据越界 / 重记与倒序 / 留痕为零 / 终账后再重放。
pub fn inject_four() -> [bool; 4] {
    // ① 判据号越界
    let mut a0 = CloseLedger::new();
    let r0 = a0.replay(CRITERIA, true, 1).err() == Some(BC_ENO)
        && a0.replay(CRITERIA.wrapping_add(6), true, 1).err() == Some(BC_ENO)
        && a0.is_empty();

    // ② 重记与倒序
    let mut a1 = CloseLedger::new();
    let s0 = a1.replay(0, true, 1).is_ok();
    let s1 = a1.replay(1, true, 1).is_ok();
    let dup = s0 && s1 && a1.replay(0, true, 1).err() == Some(BC_EDUP);
    let mut a2 = CloseLedger::new();
    let _ = a2.replay(3, true, 1);
    let order = a2.replay(2, true, 1).err() == Some(BC_EORDER);
    let r1 = dup && order;

    // ③ 留痕为零与行数不守恒
    let mut a3 = CloseLedger::new();
    let mark = a3.replay(0, true, 0).err() == Some(BC_EMARK);
    let mut a4 = CloseLedger::new();
    let mut id = 0usize;
    while id < CRITERIA {
        let _ = a4.replay(id, true, 1);
        id = id.wrapping_add(1);
    }
    let _ = a4.set_lines(BATCH_LINES.wrapping_sub(1));
    let line = a4.settle().err() == Some(BC_ELINE);
    let r2 = mark && line;

    // ④ 终账后再重放 / 再记行 / 再收口
    let (mut a5, ok5) = build_b38();
    let locked = a5.freeze().is_some();
    let r3 = ok5
        && locked
        && a5.replay(0, true, 1).err() == Some(BC_EFROZEN)
        && a5.set_lines(BATCH_LINES).err() == Some(BC_EFROZEN)
        && a5.settle().err() == Some(BC_EFROZEN);

    [r0, r1, r2, r3]
}

/// 判据：UNX-F11160 B38 批收口。
pub fn run_unx11160_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11160");
    let (mut l, ok) = build_b38();

    // X1 十九判据齐
    s.add_dyn(
        "X1-十九判据齐",
        ok && l.len() == CRITERIA && l.gaps().is_empty(),
        &format!("criteria={}/{} gaps={}", l.len(), CRITERIA, l.gaps().len()),
    );

    // X2 逐条重放全绿
    s.add_dyn(
        "X2-逐条全绿",
        l.reds() == 0 && l.all_traced(),
        &format!("reds={} traced={}", l.reds(), l.all_traced() as u8),
    );

    // X3 留痕合计
    s.add_dyn(
        "X3-留痕合计",
        l.marks_sum() == (CRITERIA as u64).wrapping_mul(3),
        &format!("marks={}/{}", l.marks_sum(), CRITERIA * 3),
    );

    // X4 本批行数
    s.add_dyn(
        "X4-本批行数",
        l.lines() == BATCH_LINES,
        &format!("lines={}/{}", l.lines(), BATCH_LINES),
    );

    // X5 行数守恒
    s.add_dyn(
        "X5-行数守恒",
        l.lines_conserved(),
        &format!("prior+lines={} domain={}", PRIOR_LINES + l.lines(), DOMAIN_LINES),
    );

    // X6 判据序严格
    s.add("X6-判据序严格", l.order_strict(), "19 条按 0..18 递增重放");

    // X7 终值即指纹
    let root = l.settled_value();
    s.add("X7-终值即指纹", root == Some(l.fold()), "收口单值等于十九判据指纹");

    // X8 冻结幂等
    let f1 = l.freeze();
    let f2 = l.freeze();
    s.add("X8-冻结幂等", f1.is_some() && f1 == f2, "二次冻结返回同一单值");

    // X9 冻结后拒重放拒记行
    s.add(
        "X9-冻结后拒动",
        l.is_frozen()
            && l.replay(0, true, 1).err() == Some(BC_EFROZEN)
            && l.set_lines(BATCH_LINES).err() == Some(BC_EFROZEN)
            && l.settle().err() == Some(BC_EFROZEN),
        "冻结后 replay/set_lines/settle 都落 BC_EFROZEN",
    );

    // X10 越界 / 重记 / 倒序 / 零痕
    let mut g = CloseLedger::new();
    let g0 = g.replay(0, true, 1).is_ok();
    let mut o = CloseLedger::new();
    let _ = o.replay(4, true, 1);
    s.add(
        "X10-四路拒止",
        g.replay(CRITERIA, true, 1).err() == Some(BC_ENO)
            && g0
            && g.replay(0, true, 1).err() == Some(BC_EDUP)
            && o.replay(3, true, 1).err() == Some(BC_EORDER)
            && g.replay(1, true, 0).err() == Some(BC_EMARK),
        "越界/重记/倒序/零痕各按其码",
    );

    // X11 行数重复记与零行
    let mut ln = CloseLedger::new();
    s.add(
        "X11-行数账两拒",
        ln.set_lines(0).err() == Some(BC_ELINE)
            && ln.set_lines(BATCH_LINES).is_ok()
            && ln.set_lines(BATCH_LINES).err() == Some(BC_EDUP)
            && ln.accepts == 1,
        "零行落 ELINE，重记行数落 EDUP",
    );

    // X12 未齐 / 有红 / 行数不守恒
    let mut q1 = CloseLedger::new();
    let mut id = 0usize;
    while id < CRITERIA - 1 {
        let _ = q1.replay(id, true, 1);
        id = id.wrapping_add(1);
    }
    let _ = q1.set_lines(BATCH_LINES);
    let mut q2 = CloseLedger::new();
    let mut id = 0usize;
    while id < CRITERIA {
        let _ = q2.replay(id, id != 7, 1);
        id = id.wrapping_add(1);
    }
    let _ = q2.set_lines(BATCH_LINES);
    let mut q3 = CloseLedger::new();
    let mut id = 0usize;
    while id < CRITERIA {
        let _ = q3.replay(id, true, 1);
        id = id.wrapping_add(1);
    }
    let _ = q3.set_lines(BATCH_LINES.wrapping_sub(1));
    let hits = inject_four();
    let all = hits[0] && hits[1] && hits[2] && hits[3];
    let (mut fresh, ok2) = build_b38();
    let root2 = fresh.freeze().unwrap_or(0);
    s.add_dyn(
        "X13-三拒与注入",
        q1.settle().err() == Some(BC_ESHORT)
            && q2.settle().err() == Some(BC_ERED)
            && q3.settle().err() == Some(BC_ELINE)
            && all
            && ok2
            && fresh.accepts == 20
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
    fn f11160_flow() {
        let (mut l, ok) = build_b38();
        assert!(ok);
        assert_eq!(l.len(), CRITERIA);
        assert!(l.gaps().is_empty());
        assert!(l.order_strict());
        assert_eq!(l.reds(), 0);
        assert!(l.all_traced());
        assert_eq!(l.lines(), BATCH_LINES);
        assert!(l.lines_conserved());
        let root = l.settled_value();
        assert_eq!(root, Some(l.fold()));
        assert_eq!(l.freeze(), root);
        assert_eq!(l.freeze(), root);
        assert!(l.is_frozen());
        assert_eq!(l.replay(0, true, 1), Err(BC_EFROZEN));
        assert_eq!(l.settle(), Err(BC_EFROZEN));
    }

    #[test]
    fn f11160_gates() {
        let mut l = CloseLedger::new();
        assert!(l.replay(0, true, 1).is_ok());
        assert!(l.replay(1, true, 1).is_ok());
        assert_eq!((l.accepts, l.rejects), (2, 0));
        assert_eq!(l.replay(CRITERIA, true, 1), Err(BC_ENO));
        assert_eq!(l.replay(0, true, 1), Err(BC_EDUP));
        assert_eq!((l.accepts, l.rejects), (2, 2));

        let mut m = CloseLedger::new();
        assert_eq!(m.replay(0, true, 0), Err(BC_EMARK));
        assert_eq!((m.accepts, m.rejects), (0, 1));

        let mut o = CloseLedger::new();
        let _ = o.replay(2, true, 1);
        assert_eq!(o.replay(1, true, 1), Err(BC_EORDER));
        assert_eq!((o.accepts, o.rejects), (1, 1));

        let mut ln = CloseLedger::new();
        assert_eq!(ln.set_lines(0), Err(BC_ELINE));
        assert!(ln.set_lines(BATCH_LINES).is_ok());
        assert_eq!(ln.set_lines(BATCH_LINES), Err(BC_EDUP));
        assert_eq!((ln.accepts, ln.rejects), (1, 2));

        let mut q = CloseLedger::new();
        let mut id = 0usize;
        while id < CRITERIA - 1 {
            let _ = q.replay(id, true, 1);
            id = id.wrapping_add(1);
        }
        let _ = q.set_lines(BATCH_LINES);
        assert_eq!(q.settle(), Err(BC_ESHORT));
        assert_eq!((q.accepts, q.rejects), (19, 1));

        let mut r = CloseLedger::new();
        let mut id = 0usize;
        while id < CRITERIA {
            let _ = r.replay(id, id != 7, 1);
            id = id.wrapping_add(1);
        }
        let _ = r.set_lines(BATCH_LINES);
        assert_eq!(r.settle(), Err(BC_ERED));
        assert_eq!((r.accepts, r.rejects), (20, 1));

        let mut k = CloseLedger::new();
        let mut id = 0usize;
        while id < CRITERIA {
            let _ = k.replay(id, true, 1);
            id = id.wrapping_add(1);
        }
        let _ = k.set_lines(BATCH_LINES.wrapping_sub(1));
        assert_eq!(k.settle(), Err(BC_ELINE));
        assert_eq!((k.accepts, k.rejects), (20, 1));

        let (mut d, ok) = build_b38();
        assert!(ok);
        assert!(d.freeze().is_some());
        assert_eq!(d.replay(0, true, 1), Err(BC_EFROZEN));
        assert_eq!(d.set_lines(BATCH_LINES), Err(BC_EFROZEN));
        assert_eq!(d.settle(), Err(BC_EFROZEN));
        assert_eq!((d.accepts, d.rejects), (20, 3));
    }

    #[test]
    fn f11160_checks_green() {
        assert!(run_unx11160_checks().all_passed());
    }
}
