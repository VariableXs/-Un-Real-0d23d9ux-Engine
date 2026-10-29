//! UNX-F11184 域字数总账终核（深化册四十册字数和）（0x1f6f0 段）
//!
//! 纯功能模型：四十册深化册各记「字数 / 行数」两本小账，域级总账要求两端
//! 逐册齐、总和精确对平，且离差超阈的册数在闸内。卷号越界、同卷重记、
//! 零值册、卷序倒挂、未齐即终核、总额不符、终核后再记，一律拒止。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f6f0;
/// 卷号越界
pub const VT_ENO: u32 = SEG_BASE + 0x01;
/// 同卷重记
pub const VT_EDUP: u32 = SEG_BASE + 0x02;
/// 零值册（字数或行数为零）
pub const VT_EZERO: u32 = SEG_BASE + 0x03;
/// 卷序倒挂
pub const VT_EORDER: u32 = SEG_BASE + 0x04;
/// 未齐即终核
pub const VT_ESHORT: u32 = SEG_BASE + 0x05;
/// 总额不符
pub const VT_ESUM: u32 = SEG_BASE + 0x06;
/// 空账
pub const VT_EEMPTY: u32 = SEG_BASE + 0x07;
/// 已终核
pub const VT_EFROZEN: u32 = SEG_BASE + 0x08;

/// 深化册卷数。
pub const VOLUMES: usize = 40;
/// 域字数总额（四十册 2,400,000 字）。
pub const DOMAIN_CHARS: u64 = 2_400_000;
/// 域行数总额（四十册 60,000 行）。
pub const DOMAIN_LINES: u64 = 60_000;
/// 离差阈值：单册偏离均值超过此值即计入超阈册。
pub const SPREAD_TOL: u32 = 12_000;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一册小账。
#[derive(Clone, Copy, Debug)]
pub struct Volume {
    pub no: u8,
    pub chars: u32,
    pub lines: u32,
}

/// 域字数总账。
#[derive(Clone, Debug)]
pub struct WordLedger {
    vols: Vec<Volume>,
    finalized: Option<u64>,
    frozen: bool,
    pub accepts: u64,
    pub rejects: u64,
    pub audits: u64,
}

impl WordLedger {
    /// 空账。
    pub fn new() -> WordLedger {
        WordLedger {
            vols: Vec::new(),
            finalized: None,
            frozen: false,
            accepts: 0,
            rejects: 0,
            audits: 0,
        }
    }

    /// 已记册数。
    pub fn len(&self) -> usize {
        self.vols.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.vols.is_empty()
    }

    /// 按卷号取册。
    pub fn volume(&self, no: usize) -> Option<Volume> {
        let mut i = 0usize;
        while i < self.vols.len() {
            if self.vols[i].no as usize == no {
                return Some(self.vols[i]);
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 记一册：终核→卷号→重记→卷序→零值。
    pub fn record(&mut self, no: usize, chars: u32, lines: u32) -> Result<u64, u32> {
        if self.frozen || self.finalized.is_some() {
            return self.fail(VT_EFROZEN);
        }
        if no >= VOLUMES {
            return self.fail(VT_ENO);
        }
        if self.volume(no).is_some() {
            return self.fail(VT_EDUP);
        }
        if let Some(last) = self.vols.last() {
            if no <= last.no as usize {
                return self.fail(VT_EORDER);
            }
        }
        if chars == 0 || lines == 0 {
            return self.fail(VT_EZERO);
        }
        self.vols.push(Volume {
            no: no as u8,
            chars,
            lines,
        });
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> Result<u64, u32> {
        self.rejects = self.rejects.wrapping_add(1);
        Err(code)
    }

    /// 字数总和。
    pub fn total_chars(&self) -> u64 {
        let mut t = 0u64;
        let mut i = 0usize;
        while i < self.vols.len() {
            t = t.wrapping_add(self.vols[i].chars as u64);
            i = i.wrapping_add(1);
        }
        t
    }

    /// 行数总和。
    pub fn total_lines(&self) -> u64 {
        let mut t = 0u64;
        let mut i = 0usize;
        while i < self.vols.len() {
            t = t.wrapping_add(self.vols[i].lines as u64);
            i = i.wrapping_add(1);
        }
        t
    }

    /// 字数均值（向下取整；空账为零）。
    pub fn mean_chars(&self) -> u32 {
        if self.vols.is_empty() {
            return 0;
        }
        (self.total_chars() / self.vols.len() as u64) as u32
    }

    /// 离差超阈的册数。
    pub fn outliers(&self, tol: u32) -> usize {
        let mean = self.mean_chars();
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.vols.len() {
            let c = self.vols[i].chars;
            let d = if c > mean {
                c.wrapping_sub(mean)
            } else {
                mean.wrapping_sub(c)
            };
            if d > tol {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 逐卷齐备（0..VOLUMES 无缺号）。
    pub fn complete(&self) -> bool {
        if self.vols.len() != VOLUMES {
            return false;
        }
        let mut n = 0usize;
        while n < VOLUMES {
            if self.volume(n).is_none() {
                return false;
            }
            n = n.wrapping_add(1);
        }
        true
    }

    /// 缺口卷号表。
    pub fn gaps(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut n = 0usize;
        while n < VOLUMES {
            if self.volume(n).is_none() {
                out.push(n as u32);
            }
            n = n.wrapping_add(1);
        }
        out
    }

    /// 总账指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.vols.len() {
            let v = self.vols[i];
            h = (h ^ v.no as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ v.chars as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ v.lines as u64).wrapping_mul(FNV_PRIME);
            i = i.wrapping_add(1);
        }
        h = (h ^ self.vols.len() as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.total_chars()).wrapping_mul(FNV_PRIME);
        h = (h ^ self.total_lines()).wrapping_mul(FNV_PRIME);
        h
    }

    /// 终核：四十册齐、字数与行数两端精确对平。
    pub fn finalize(&mut self, expected_chars: u64, expected_lines: u64) -> Result<u64, u32> {
        self.audits = self.audits.wrapping_add(1);
        if self.finalized.is_some() {
            return self.fail(VT_EFROZEN);
        }
        if self.vols.is_empty() {
            return self.fail(VT_EEMPTY);
        }
        if !self.complete() {
            return self.fail(VT_ESHORT);
        }
        if self.total_chars() != expected_chars || self.total_lines() != expected_lines {
            return self.fail(VT_ESUM);
        }
        let v = self.fold();
        self.finalized = Some(v);
        Ok(v)
    }

    /// 冻结：锁当下单值，幂等。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.finalized;
        }
        if self.finalized.is_none() {
            self.finalized = Some(self.fold());
        }
        self.frozen = true;
        self.finalized
    }

    /// 终核值。
    pub fn final_value(&self) -> Option<u64> {
        self.finalized
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 建账：四十册均分域字数与域行数，两本小账各自对平。
pub fn build_ledger() -> (WordLedger, bool) {
    let mut l = WordLedger::new();
    let chars = (DOMAIN_CHARS / VOLUMES as u64) as u32;
    let lines = (DOMAIN_LINES / VOLUMES as u64) as u32;
    let mut n = 0usize;
    while n < VOLUMES {
        if l.record(n, chars, lines).is_err() {
            return (l, false);
        }
        n = n.wrapping_add(1);
    }
    let ok = l.finalize(DOMAIN_CHARS, DOMAIN_LINES).is_ok();
    (l, ok)
}

/// 四类注入：卷号越界 / 同卷重记 / 零值册 / 总额不符与终核后登记。
pub fn inject_four() -> [bool; 4] {
    // ① 卷号越界
    let mut a0 = WordLedger::new();
    let r0 = a0.record(VOLUMES, 1, 1).err() == Some(VT_ENO)
        && a0.record(VOLUMES.wrapping_add(7), 1, 1).err() == Some(VT_ENO);

    // ② 同卷重记与卷序倒挂
    let mut a1 = WordLedger::new();
    let s1 = a1.record(0, 60_000, 1_500).is_ok();
    let s1b = a1.record(1, 60_000, 1_500).is_ok();
    let r1 = s1
        && s1b
        && a1.record(0, 60_000, 1_500).err() == Some(VT_EDUP)
        && a1.record(2, 0, 1_500).err() == Some(VT_EZERO);

    // ③ 总额不符与未齐即终核
    let mut a2 = WordLedger::new();
    let empty = a2.finalize(DOMAIN_CHARS, DOMAIN_LINES).err() == Some(VT_EEMPTY);
    let mut a3 = WordLedger::new();
    let _ = a3.record(0, 60_000, 1_500);
    let short = a3.finalize(DOMAIN_CHARS, DOMAIN_LINES).err() == Some(VT_ESHORT);
    let (mut a4, ok4) = build_ledger_without_final();
    let off = a4
        .finalize(DOMAIN_CHARS.wrapping_add(1), DOMAIN_LINES)
        .err()
        == Some(VT_ESUM);
    let r2 = empty && short && ok4 && off;

    // ④ 终核后登记与二次终核
    let (mut a5, ok5) = build_ledger();
    let locked = a5.freeze().is_some();
    let r3 = ok5
        && locked
        && a5.record(0, 60_000, 1_500).err() == Some(VT_EFROZEN)
        && a5.finalize(DOMAIN_CHARS, DOMAIN_LINES).err() == Some(VT_EFROZEN);

    [r0, r1, r2, r3]
}

/// 建账但不终核（供总额不符用例使用）。
pub fn build_ledger_without_final() -> (WordLedger, bool) {
    let mut l = WordLedger::new();
    let chars = (DOMAIN_CHARS / VOLUMES as u64) as u32;
    let lines = (DOMAIN_LINES / VOLUMES as u64) as u32;
    let mut n = 0usize;
    while n < VOLUMES {
        if l.record(n, chars, lines).is_err() {
            return (l, false);
        }
        n = n.wrapping_add(1);
    }
    (l, true)
}

/// 判据：UNX-F11184 域字数总账终核。
pub fn run_unx11184_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11184");
    let (mut l, ok) = build_ledger();

    // V1 四十册齐备
    s.add_dyn(
        "V1-四十册齐备",
        ok && l.complete() && l.len() == VOLUMES && l.gaps().is_empty(),
        &format!("vols={}/{} gaps={}", l.len(), VOLUMES, l.gaps().len()),
    );

    // V2 字数总额对平
    s.add_dyn(
        "V2-字数总额对平",
        l.total_chars() == DOMAIN_CHARS,
        &format!("chars={}/{}", l.total_chars(), DOMAIN_CHARS),
    );

    // V3 行数总额对平
    s.add_dyn(
        "V3-行数总额对平",
        l.total_lines() == DOMAIN_LINES,
        &format!("lines={}/{}", l.total_lines(), DOMAIN_LINES),
    );

    // V4 两本小账同向（每册字数行数皆非零且比值合理）
    let mut same_dir = true;
    let mut i = 0usize;
    while i < VOLUMES {
        match l.volume(i) {
            Some(v) => {
                if v.chars == 0 || v.lines == 0 {
                    same_dir = false;
                }
                // 每行至少 20 字，最多 200 字：畸变册即失向
                let per_line = v.chars / v.lines;
                if per_line < 20 || per_line > 200 {
                    same_dir = false;
                }
            }
            None => same_dir = false,
        }
        i = i.wrapping_add(1);
    }
    s.add("V4-两账同向", same_dir, "每册每行 20~200 字，字数账与行数账同向");

    // V5 均值即均分
    s.add_dyn(
        "V5-均值即均分",
        l.mean_chars() == (DOMAIN_CHARS / VOLUMES as u64) as u32 && l.outliers(SPREAD_TOL) == 0,
        &format!("mean={} outliers={}", l.mean_chars(), l.outliers(SPREAD_TOL)),
    );

    // V6 终核值即指纹
    let root = l.final_value();
    s.add(
        "V6-终核值即指纹",
        root == Some(l.fold()) && l.freeze() == root,
        "终核值等于总账指纹，冻结不改变它",
    );

    // V7 冻结幂等
    let f1 = l.freeze();
    let f2 = l.freeze();
    s.add("V7-冻结幂等", f1.is_some() && f1 == f2, "二次冻结返回同一单值");

    // V8 终核后拒记
    s.add(
        "V8-终核后拒记",
        l.is_frozen()
            && l.record(0, 60_000, 1_500).err() == Some(VT_EFROZEN)
            && l.finalize(DOMAIN_CHARS, DOMAIN_LINES).err() == Some(VT_EFROZEN),
        "终核后既不许记册也不许再核",
    );

    // V9 空账拒终核
    let mut e = WordLedger::new();
    s.add(
        "V9-空账拒终核",
        e.finalize(DOMAIN_CHARS, DOMAIN_LINES).err() == Some(VT_EEMPTY) && e.is_empty(),
        "空账不得终核",
    );

    // V10 未齐拒终核
    let mut h = WordLedger::new();
    let mut i = 0usize;
    while i < VOLUMES - 1 {
        let _ = h.record(i, 60_000, 1_500);
        i = i.wrapping_add(1);
    }
    s.add_dyn(
        "V10-未齐拒终核",
        h.finalize(DOMAIN_CHARS, DOMAIN_LINES).err() == Some(VT_ESHORT)
            && h.gaps() == alloc::vec![(VOLUMES - 1) as u32],
        &format!("vols={} gaps={}", h.len(), h.gaps().len()),
    );

    // V11 总额不符拒终核
    let (mut o, ok_o) = build_ledger_without_final();
    s.add(
        "V11-总额不符拒终核",
        ok_o && o.finalize(DOMAIN_CHARS.wrapping_add(40), DOMAIN_LINES).err() == Some(VT_ESUM),
        "字数总额差一册即拒",
    );

    // V12 越界重记零值拒止
    let mut g = WordLedger::new();
    let g0 = g.record(0, 60_000, 1_500).is_ok();
    s.add(
        "V12-越界重记零值拒止",
        g.record(VOLUMES, 1, 1).err() == Some(VT_ENO)
            && g0
            && g.record(0, 60_000, 1_500).err() == Some(VT_EDUP)
            && g.record(1, 0, 1).err() == Some(VT_EZERO),
        "越界/重记/零值三路各按其码",
    );

    // V13 注入四拒 + 终盘明细（新账重算）
    let hits = inject_four();
    let mut all = true;
    let mut k = 0usize;
    while k < hits.len() {
        if !hits[k] {
            all = false;
        }
        k = k.wrapping_add(1);
    }
    let (mut fresh, ok2) = build_ledger();
    let root2 = fresh.freeze().unwrap_or(0);
    s.add_dyn(
        "V13-注入四拒与终盘明细",
        all && ok2 && fresh.rejects == 0 && fresh.accepts == VOLUMES as u64 && root2 != 0,
        &format!(
            "inject={}{}{}{} acc={} rej={} chars={} root={:#x}",
            hits[0] as u8, hits[1] as u8, hits[2] as u8, hits[3] as u8,
            fresh.accepts, fresh.rejects, fresh.total_chars(), root2
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11184_ledger_flow() {
        let (mut l, ok) = build_ledger();
        assert!(ok);
        assert!(l.complete());
        assert_eq!(l.total_chars(), DOMAIN_CHARS);
        assert_eq!(l.total_lines(), DOMAIN_LINES);
        assert_eq!(l.outliers(SPREAD_TOL), 0);
        let root = l.final_value();
        assert_eq!(root, Some(l.fold()));
        assert_eq!(l.freeze(), root);
        assert_eq!(l.freeze(), root);
        assert_eq!(l.record(0, 1, 1), Err(VT_EFROZEN));
    }

    #[test]
    fn f11184_gates() {
        let mut l = WordLedger::new();
        assert_eq!(l.record(VOLUMES, 1, 1), Err(VT_ENO));
        assert_eq!(l.record(0, 0, 1), Err(VT_EZERO));
        assert!(l.record(0, 60_000, 1_500).is_ok());
        assert_eq!(l.record(0, 60_000, 1_500), Err(VT_EDUP));
        assert_eq!(l.record(2, 60_000, 1_500), Ok(l.fold()));
        assert_eq!(l.record(1, 60_000, 1_500), Err(VT_EORDER));
        assert_eq!(l.rejects, 4);
        assert_eq!(l.accepts, 2);
    }

    #[test]
    fn f11184_checks_green() {
        let cs = run_unx11184_checks();
        assert!(cs.all_passed());
    }
}
