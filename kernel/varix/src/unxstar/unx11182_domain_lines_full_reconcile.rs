//! UNX-F11182 域 240,000 行满账终对账（四十批和终验）（0x1f700 段）
//!
//! 纯功能模型：域行数账按批记，四十批、批批 6,000 行、批批 20 判据。终对账
//! 不只核总和，还核「累进步长」——每加一批前缀和恰增 6,000；某批缺席、
//! 重批、乱批、批内行数不足、判据数偏离，都会让累进链断在那一批上。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f700;
/// 批号越界
pub const DR_ENO: u32 = SEG_BASE + 0x01;
/// 重批
pub const DR_EDUP: u32 = SEG_BASE + 0x02;
/// 批序倒挂
pub const DR_EORDER: u32 = SEG_BASE + 0x03;
/// 批内行数或判据为零
pub const DR_EZERO: u32 = SEG_BASE + 0x04;
/// 未齐即对账
pub const DR_ESHORT: u32 = SEG_BASE + 0x05;
/// 总额不符
pub const DR_ESUM: u32 = SEG_BASE + 0x06;
/// 批间步长不齐
pub const DR_ESTEP: u32 = SEG_BASE + 0x07;
/// 已终账
pub const DR_EFROZEN: u32 = SEG_BASE + 0x08;

/// 域批数。
pub const BATCHES: usize = 40;
/// 每批行数。
pub const LINES_PER_BATCH: u32 = 6_000;
/// 每批判据数。
pub const CRITERIA_PER_BATCH: u16 = 20;
/// 域满账行数。
pub const DOMAIN_LINES: u64 = 240_000;
/// 域判据总数。
pub const DOMAIN_CRITERIA: u64 = 800;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一批行数账。
#[derive(Clone, Copy, Debug)]
pub struct Batch {
    pub no: u8,
    pub lines: u32,
    pub criteria: u16,
}

/// 域行数满账。
#[derive(Clone, Debug)]
pub struct LinesLedger {
    batches: Vec<Batch>,
    settled: Option<u64>,
    frozen: bool,
    pub accepts: u64,
    pub rejects: u64,
}

impl LinesLedger {
    /// 空账。
    pub fn new() -> LinesLedger {
        LinesLedger {
            batches: Vec::new(),
            settled: None,
            frozen: false,
            accepts: 0,
            rejects: 0,
        }
    }

    /// 已记批数。
    pub fn len(&self) -> usize {
        self.batches.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.batches.is_empty()
    }

    /// 取某批。
    pub fn batch(&self, no: usize) -> Option<Batch> {
        let mut i = 0usize;
        while i < self.batches.len() {
            if self.batches[i].no as usize == no {
                return Some(self.batches[i]);
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 记一批：终账→批号→重批→批序→零值。
    pub fn record(&mut self, no: usize, lines: u32, criteria: u16) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(DR_EFROZEN);
        }
        if no >= BATCHES {
            return self.fail(DR_ENO);
        }
        if self.batch(no).is_some() {
            return self.fail(DR_EDUP);
        }
        if let Some(last) = self.batches.last() {
            if no <= last.no as usize {
                return self.fail(DR_EORDER);
            }
        }
        if lines == 0 || criteria == 0 {
            return self.fail(DR_EZERO);
        }
        self.batches.push(Batch {
            no: no as u8,
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

    /// 行数前缀和表（逐批累进）。
    pub fn running_lines(&self) -> Vec<u64> {
        let mut out: Vec<u64> = Vec::new();
        let mut acc = 0u64;
        let mut i = 0usize;
        while i < self.batches.len() {
            acc = acc.wrapping_add(self.batches[i].lines as u64);
            out.push(acc);
            i = i.wrapping_add(1);
        }
        out
    }

    /// 行数总和。
    pub fn total_lines(&self) -> u64 {
        let mut t = 0u64;
        let mut i = 0usize;
        while i < self.batches.len() {
            t = t.wrapping_add(self.batches[i].lines as u64);
            i = i.wrapping_add(1);
        }
        t
    }

    /// 判据总和。
    pub fn total_criteria(&self) -> u64 {
        let mut t = 0u64;
        let mut i = 0usize;
        while i < self.batches.len() {
            t = t.wrapping_add(self.batches[i].criteria as u64);
            i = i.wrapping_add(1);
        }
        t
    }

    /// 批间步长齐：前缀和逐格恰增 LINES_PER_BATCH。
    pub fn steps_uniform(&self) -> bool {
        let run = self.running_lines();
        let mut i = 0usize;
        while i < run.len() {
            let want = (i as u64).wrapping_add(1).saturating_mul(LINES_PER_BATCH as u64);
            match run.get(i) {
                Some(v) => {
                    if *v != want {
                        return false;
                    }
                }
                None => return false,
            }
            i = i.wrapping_add(1);
        }
        run.len() == BATCHES
    }

    /// 批批同形：每批行数与判据数都等于口径值。
    pub fn uniform_batches(&self) -> bool {
        let mut i = 0usize;
        while i < self.batches.len() {
            let b = self.batches[i];
            if b.lines != LINES_PER_BATCH || b.criteria != CRITERIA_PER_BATCH {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 缺批表。
    pub fn gaps(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut n = 0usize;
        while n < BATCHES {
            if self.batch(n).is_none() {
                out.push(n as u32);
            }
            n = n.wrapping_add(1);
        }
        out
    }

    /// 终账指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.batches.len() {
            let b = self.batches[i];
            h = (h ^ b.no as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ b.lines as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ b.criteria as u64).wrapping_mul(FNV_PRIME);
            i = i.wrapping_add(1);
        }
        h = (h ^ self.batches.len() as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.total_lines()).wrapping_mul(FNV_PRIME);
        (h ^ self.total_criteria()).wrapping_mul(FNV_PRIME)
    }

    /// 终对账：四十批齐 → 总额对平 → 批批同形 → 步长齐。
    pub fn reconcile(&mut self) -> Result<u64, u32> {
        if self.settled.is_some() {
            return self.fail(DR_EFROZEN);
        }
        if self.batches.is_empty() {
            return self.fail(DR_ESHORT);
        }
        if self.batches.len() != BATCHES || !self.gaps().is_empty() {
            return self.fail(DR_ESHORT);
        }
        if self.total_lines() != DOMAIN_LINES || self.total_criteria() != DOMAIN_CRITERIA {
            return self.fail(DR_ESUM);
        }
        // 步长先于同形裁定：总量对平而批间挪行的情形要落 ESTEP 而非 ESUM。
        if !self.steps_uniform() {
            return self.fail(DR_ESTEP);
        }
        if !self.uniform_batches() {
            return self.fail(DR_ESUM);
        }
        let v = self.fold();
        self.settled = Some(v);
        Ok(v)
    }

    /// 冻结：锁单值，幂等。
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

    /// 终账值。
    pub fn settled_value(&self) -> Option<u64> {
        self.settled
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 建账：四十批各 6,000 行、20 判据，再终对账。
pub fn build_lines() -> (LinesLedger, bool) {
    let mut l = LinesLedger::new();
    let mut n = 0usize;
    while n < BATCHES {
        if l.record(n, LINES_PER_BATCH, CRITERIA_PER_BATCH).is_err() {
            return (l, false);
        }
        n = n.wrapping_add(1);
    }
    let ok = l.reconcile().is_ok();
    (l, ok)
}

/// 四类注入：批号越界 / 重批与乱批 / 步长断链与总额不符 / 终账后再记。
pub fn inject_four() -> [bool; 4] {
    // ① 批号越界
    let mut a0 = LinesLedger::new();
    let r0 = a0.record(BATCHES, 1, 1).err() == Some(DR_ENO)
        && a0.record(BATCHES.wrapping_add(4), 1, 1).err() == Some(DR_ENO);

    // ② 重批、乱批、零值批
    let mut a1 = LinesLedger::new();
    let s1 = a1.record(0, LINES_PER_BATCH, CRITERIA_PER_BATCH).is_ok();
    let s2 = a1.record(1, LINES_PER_BATCH, CRITERIA_PER_BATCH).is_ok();
    let r1 = s1
        && s2
        && a1.record(0, LINES_PER_BATCH, CRITERIA_PER_BATCH).err() == Some(DR_EDUP)
        && a1.record(3, LINES_PER_BATCH, CRITERIA_PER_BATCH).is_ok()
        && a1.record(2, LINES_PER_BATCH, CRITERIA_PER_BATCH).err() == Some(DR_EORDER)
        && a1.record(4, 0, CRITERIA_PER_BATCH).err() == Some(DR_EZERO);

    // ③ 步长断链 / 总额不符 / 未齐
    let mut a2 = LinesLedger::new();
    let empty = a2.reconcile().err() == Some(DR_ESHORT);
    let mut a3 = LinesLedger::new();
    let mut i = 0usize;
    while i < BATCHES {
        let _ = a3.record(i, LINES_PER_BATCH, CRITERIA_PER_BATCH);
        i = i.wrapping_add(1);
    }
    // 把第 0 批的行数挪走 1 行：总和仍可能对平但步长必断
    let mut a4 = LinesLedger::new();
    let mut i = 0usize;
    while i < BATCHES {
        let lines = if i == 5 {
            LINES_PER_BATCH.wrapping_add(1)
        } else if i == 6 {
            LINES_PER_BATCH.wrapping_sub(1)
        } else {
            LINES_PER_BATCH
        };
        let _ = a4.record(i, lines, CRITERIA_PER_BATCH);
        i = i.wrapping_add(1);
    }
    let sum_ok_but_step_bad = a4.reconcile().err() == Some(DR_ESTEP);
    let mut a5 = LinesLedger::new();
    let mut i = 0usize;
    while i < BATCHES - 1 {
        let _ = a5.record(i, LINES_PER_BATCH, CRITERIA_PER_BATCH);
        i = i.wrapping_add(1);
    }
    let short = a5.reconcile().err() == Some(DR_ESHORT);
    let r2 = empty && sum_ok_but_step_bad && short;

    // ④ 终账后再记 / 二次对账
    let (mut a6, ok6) = build_lines();
    let locked = a6.freeze().is_some();
    let r3 = ok6
        && locked
        && a6.record(0, LINES_PER_BATCH, CRITERIA_PER_BATCH).err() == Some(DR_EFROZEN)
        && a6.reconcile().err() == Some(DR_EFROZEN);

    [r0, r1, r2, r3]
}

/// 判据：UNX-F11182 域满账行数终对账。
pub fn run_unx11182_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11182");
    let (mut l, ok) = build_lines();

    // D1 四十批齐备
    s.add_dyn(
        "D1-四十批齐备",
        ok && l.len() == BATCHES && l.gaps().is_empty(),
        &format!("batches={}/{} gaps={}", l.len(), BATCHES, l.gaps().len()),
    );

    // D2 行数总额对平
    s.add_dyn(
        "D2-行数总额对平",
        l.total_lines() == DOMAIN_LINES,
        &format!("lines={}/{}", l.total_lines(), DOMAIN_LINES),
    );

    // D3 判据总额对平
    s.add_dyn(
        "D3-判据总额对平",
        l.total_criteria() == DOMAIN_CRITERIA,
        &format!("criteria={}/{}", l.total_criteria(), DOMAIN_CRITERIA),
    );

    // D4 批批同形
    s.add(
        "D4-批批同形",
        l.uniform_batches(),
        "每批 6,000 行、20 判据，四十批同形",
    );

    // D5 步长齐
    let run = l.running_lines();
    let last = run.last().copied().unwrap_or(0);
    s.add_dyn(
        "D5-批间步长齐",
        l.steps_uniform() && run.len() == BATCHES && last == DOMAIN_LINES,
        &format!("prefix_last={} steps={}", last, run.len()),
    );

    // D6 终账值即指纹
    let root = l.settled_value();
    s.add(
        "D6-终账值即指纹",
        root == Some(l.fold()) && l.freeze() == root,
        "终账值等于满账指纹，冻结不改变它",
    );

    // D7 冻结幂等
    let f1 = l.freeze();
    let f2 = l.freeze();
    s.add("D7-冻结幂等", f1.is_some() && f1 == f2, "二次冻结返回同一单值");

    // D8 终账后拒记
    s.add(
        "D8-终账后拒记",
        l.is_frozen()
            && l.record(0, LINES_PER_BATCH, CRITERIA_PER_BATCH).err() == Some(DR_EFROZEN)
            && l.reconcile().err() == Some(DR_EFROZEN),
        "终账后既不许记批也不许再对账",
    );

    // D9 空账拒对账
    let mut e = LinesLedger::new();
    s.add(
        "D9-空账拒对账",
        e.reconcile().err() == Some(DR_ESHORT) && e.is_empty(),
        "空账不得终对账",
    );

    // D10 未齐拒对账
    let mut h = LinesLedger::new();
    let mut i = 0usize;
    while i < BATCHES - 1 {
        let _ = h.record(i, LINES_PER_BATCH, CRITERIA_PER_BATCH);
        i = i.wrapping_add(1);
    }
    s.add_dyn(
        "D10-未齐拒对账",
        h.reconcile().err() == Some(DR_ESHORT) && h.gaps().len() == 1,
        &format!("batches={} gaps={}", h.len(), h.gaps().len()),
    );

    // D11 总额不符拒对账
    let mut m = LinesLedger::new();
    let mut i = 0usize;
    while i < BATCHES {
        let lines = if i == 0 {
            LINES_PER_BATCH.wrapping_sub(1)
        } else {
            LINES_PER_BATCH
        };
        let _ = m.record(i, lines, CRITERIA_PER_BATCH);
        i = i.wrapping_add(1);
    }
    s.add_dyn(
        "D11-总额不符拒对账",
        m.reconcile().err() == Some(DR_ESUM),
        &format!("lines={}/{}", m.total_lines(), DOMAIN_LINES),
    );

    // D12 越界重批零值拒止
    let mut g = LinesLedger::new();
    let g0 = g.record(0, LINES_PER_BATCH, CRITERIA_PER_BATCH).is_ok();
    s.add(
        "D12-越界重批零值拒止",
        g.record(BATCHES, 1, 1).err() == Some(DR_ENO)
            && g0
            && g.record(0, LINES_PER_BATCH, CRITERIA_PER_BATCH).err() == Some(DR_EDUP)
            && g.record(1, 0, 1).err() == Some(DR_EZERO),
        "越界/重批/零值三路各按其码",
    );

    // D13 注入四拒 + 终盘明细（新账重算）
    let hits = inject_four();
    let mut all = true;
    let mut k = 0usize;
    while k < hits.len() {
        if !hits[k] {
            all = false;
        }
        k = k.wrapping_add(1);
    }
    let (mut fresh, ok2) = build_lines();
    let root2 = fresh.freeze().unwrap_or(0);
    s.add_dyn(
        "D13-注入四拒与终盘明细",
        all && ok2 && fresh.rejects == 0 && fresh.accepts == BATCHES as u64 && root2 != 0,
        &format!(
            "inject={}{}{}{} acc={} rej={} lines={} root={:#x}",
            hits[0] as u8, hits[1] as u8, hits[2] as u8, hits[3] as u8,
            fresh.accepts, fresh.rejects, fresh.total_lines(), root2
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11182_lines_flow() {
        let (mut l, ok) = build_lines();
        assert!(ok);
        assert_eq!(l.len(), BATCHES);
        assert_eq!(l.total_lines(), DOMAIN_LINES);
        assert_eq!(l.total_criteria(), DOMAIN_CRITERIA);
        assert!(l.uniform_batches());
        assert!(l.steps_uniform());
        let root = l.settled_value();
        assert_eq!(root, Some(l.fold()));
        assert_eq!(l.freeze(), root);
        assert_eq!(l.freeze(), root);
        assert_eq!(l.record(0, 1, 1), Err(DR_EFROZEN));
    }

    #[test]
    fn f11182_step_break() {
        let mut l = LinesLedger::new();
        let mut i = 0usize;
        while i < BATCHES {
            let lines = if i == 3 {
                LINES_PER_BATCH.wrapping_add(2)
            } else if i == 4 {
                LINES_PER_BATCH.wrapping_sub(2)
            } else {
                LINES_PER_BATCH
            };
            let _ = l.record(i, lines, CRITERIA_PER_BATCH);
            i = i.wrapping_add(1);
        }
        assert_eq!(l.total_lines(), DOMAIN_LINES);
        assert!(!l.steps_uniform());
        assert_eq!(l.reconcile(), Err(DR_ESTEP));
    }

    #[test]
    fn f11182_checks_green() {
        let cs = run_unx11182_checks();
        assert!(cs.all_passed());
    }
}
