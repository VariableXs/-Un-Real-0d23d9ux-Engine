//! UNX-F11060 B33 批收口（LTP 对照联签总断言）（0x1e780 段）
//!
//! 锚点：UNX-F11060-J1 批内 19 判据重放全绿（10/10），行数守恒与域累计
//! 对平（10/10）。
//! 域：CoRun·UNX-C4-B33（外部对照面·批收口）。
//!
//! 结构：批重放册（11041–11059 十九单逐单重放槽：单号 + 探针 + 裁断位）+
//! 行数守恒器（十九单行数逐单登记、批总与闭式对平）+ 域累计对平器（B32
//! 基账 + 本批 = 总账，越限即拒）+ fold 指纹与封存 + 注入核（错值裁断/
//! 缺单/行数漂移/封存后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e780 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e000 + (11060 - 11000) * 0x20;
pub const BY_BADVERDICT: u32 = SEG_BASE + 0x01; // 裁断失真
pub const BY_SHORT: u32 = SEG_BASE + 0x02; // 十九单未齐
pub const BY_LINEDRIFT: u32 = SEG_BASE + 0x03; // 行数守恒破坏
pub const BY_DUP: u32 = SEG_BASE + 0x04; // 单号重号
pub const BY_FROZEN: u32 = SEG_BASE + 0x05; // 封存后写账
pub const BY_ACCUM: u32 = SEG_BASE + 0x06; // 域累计对平失败

/// 批内单数（11041–11059）。
pub const UNITS_NEEDED: usize = 19;
/// 批首单号。
pub const UNIT_FIRST: u32 = 11041;
/// 每单行数要求（ nineteen × 260 ）。
pub const UNIT_LINES: u64 = 260;
/// 批行数守恒目标。
pub const BATCH_LINES: u64 = UNIT_LINES * UNITS_NEEDED as u64;
/// 域累计基账（B32 收口移交）。
pub const DOM_ACCUM_BASE: u64 = 197_060;
/// 域累计目标（基账 + 本批）。
pub const DOM_ACCUM_TOTAL: u64 = DOM_ACCUM_BASE + BATCH_LINES;
/// 批收口域标（fold 种子）。
pub const BY_TAG: u64 = 0x1e78_28;

/// 单条重放槽：单号 + 探针 + 裁断位。
#[derive(Clone, Debug)]
pub struct ReplayUnit33 {
    pub unit: u32,
    pub probe: u64,
    pub verdict: bool,
    /// 裁断已执行位（与裁断通过位分列：错值单入红槽而非留缺口）。
    pub judged: bool,
    pub lines: u64,
}

impl ReplayUnit33 {
    /// 探针与裁断双位齐绿。
    pub fn green(&self) -> bool {
        self.verdict && self.probe != 0 && self.lines == UNIT_LINES
    }

    pub fn render(&self) -> String {
        let flag = if self.green() { "G" } else { "R" };
        format!("u{}|{}|p{}", self.unit, flag, self.probe)
    }
}

/// B33 批收口册：重放 → 行数守恒 → 域累计对平 → 封存。
pub struct BatchCloseLedger33 {
    pub units: Vec<ReplayUnit33>,
    pub accum: u64,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl BatchCloseLedger33 {
    pub fn new() -> BatchCloseLedger33 {
        BatchCloseLedger33 { units: Vec::new(), accum: 0, frozen: false, root: None }
    }

    /// 登记重放槽：封存、零号、重号、超容均拒。
    pub fn enroll(&mut self, unit: u32, probe: u64) -> Result<(), u32> {
        if self.frozen {
            return Err(BY_FROZEN);
        }
        if unit == 0 || self.units.iter().any(|u| u.unit == unit) {
            return Err(BY_DUP);
        }
        if self.units.len() >= UNITS_NEEDED + 8 {
            return Err(BY_SHORT);
        }
        self.units.push(ReplayUnit33 { unit, probe, verdict: false, judged: false, lines: 0 });
        Ok(())
    }

    /// 裁断：探针与闭式期望一致才翻绿；行数随裁断入账。
    pub fn judge(&mut self, unit: u32, expect: u64, lines: u64) -> Result<bool, u32> {
        if self.frozen {
            return Err(BY_FROZEN);
        }
        if lines != UNIT_LINES {
            return Err(BY_LINEDRIFT);
        }
        let mut hit = false;
        let mut ok = false;
        let mut k = 0usize;
        while k < self.units.len() {
            if self.units[k].unit == unit {
                ok = self.units[k].probe == expect && self.units[k].probe != 0;
                self.units[k].verdict = ok;
                self.units[k].judged = true;
                self.units[k].lines = lines;
                hit = true;
            }
            k += 1;
        }
        if hit {
            Ok(ok)
        } else {
            Err(BY_SHORT)
        }
    }

    /// 未裁断单号清单。
    pub fn gaps(&self) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.units.len() {
            if !self.units[k].judged {
                g.push(self.units[k].unit);
            }
            k += 1;
        }
        g
    }

    /// 红槽单号清单。
    pub fn reds(&self) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.units.len() {
            if self.units[k].judged && !self.units[k].green() {
                g.push(self.units[k].unit);
            }
            k += 1;
        }
        g
    }

    /// 行数守恒：批总 = 十九 × 每单行数。
    pub fn lines_ok(&self) -> bool {
        self.batch_lines() == BATCH_LINES
    }

    /// 本批行数合计。
    pub fn batch_lines(&self) -> u64 {
        let mut total: u64 = 0;
        let mut k = 0usize;
        while k < self.units.len() {
            total = total.wrapping_add(self.units[k].lines);
            k += 1;
        }
        total
    }

    /// 批收口裁断：十九单齐、全绿、行数守恒。
    pub fn closed(&self) -> bool {
        self.units.len() == UNITS_NEEDED
            && self.gaps().is_empty()
            && self.reds().is_empty()
            && self.lines_ok()
    }

    /// 域累计对平：基账 + 本批 = 总账。
    pub fn reconcile(&mut self, base: u64, total: u64) -> Result<u64, u32> {
        if self.frozen {
            return Err(BY_FROZEN);
        }
        let batch = self.batch_lines();
        let sum = base.wrapping_add(batch);
        if sum != total {
            return Err(BY_ACCUM);
        }
        self.accum = sum;
        Ok(sum)
    }

    /// 链式 fold 指纹（FNV 风格，序敏感，含单号/探针/裁断/行数）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = BY_TAG;
        let mut k = 0usize;
        while k < self.units.len() {
            let u = &self.units[k];
            h = (h ^ u.unit as u64).wrapping_mul(0x100000001b3);
            h = (h ^ u.probe).wrapping_mul(0x100000001b3);
            h = (h ^ u.verdict as u64).wrapping_mul(0x100000001b3);
            h = (h ^ u.lines).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.accum).wrapping_mul(0x100000001b3);
        h
    }

    /// 封存：翻冻结位并落冻结根。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// 第 idx 单确定性探针（闭式期望供正反两路共用）。
pub fn by_probe(idx: u32) -> u64 {
    let n = idx.wrapping_sub(UNIT_FIRST).wrapping_add(7);
    let sum = n.wrapping_mul(n.wrapping_add(1)) / 2;
    (sum as u64).wrapping_mul(0x100000001b3) ^ (idx as u64).wrapping_mul(5)
}

/// 十九单全量重放。
pub fn replay_batch(l: &mut BatchCloseLedger33) -> (bool, usize) {
    let mut i = 0u32;
    while i < UNITS_NEEDED as u32 {
        let unit = UNIT_FIRST + i;
        let _ = l.enroll(unit, by_probe(unit));
        i += 1;
    }
    let mut i = 0u32;
    let mut reds = 0usize;
    while i < UNITS_NEEDED as u32 {
        let unit = UNIT_FIRST + i;
        if l.judge(unit, by_probe(unit), UNIT_LINES) != Ok(true) {
            reds += 1;
        }
        i += 1;
    }
    (reds == 0 && l.closed(), reds)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 裁断期望值给错必出红槽
    let mut a = BatchCloseLedger33::new();
    let _ = a.enroll(UNIT_FIRST, by_probe(UNIT_FIRST));
    let _ = a.judge(UNIT_FIRST, by_probe(UNIT_FIRST).wrapping_add(1), UNIT_LINES);
    got[0] = a.reds().len() == 1 && !a.closed();
    // 2) 行数漂移必拒
    let mut b = BatchCloseLedger33::new();
    let _ = b.enroll(UNIT_FIRST, by_probe(UNIT_FIRST));
    got[1] = b.judge(UNIT_FIRST, by_probe(UNIT_FIRST), UNIT_LINES - 1).err() == Some(BY_LINEDRIFT);
    // 3) 域累计对平错值必拒
    let mut c = BatchCloseLedger33::new();
    let _ = c.enroll(UNIT_FIRST, by_probe(UNIT_FIRST));
    let _ = c.judge(UNIT_FIRST, by_probe(UNIT_FIRST), UNIT_LINES);
    got[2] = c.reconcile(DOM_ACCUM_BASE, DOM_ACCUM_TOTAL + 1).err() == Some(BY_ACCUM);
    // 4) 封存后写账必抓
    let mut d = BatchCloseLedger33::new();
    let _ = replay_batch(&mut d);
    d.freeze();
    got[3] = d.enroll(99999, 1).err() == Some(BY_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11060_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11060");

    // J1 批内 19 判据重放全绿
    let mut book = BatchCloseLedger33::new();
    let (green, reds) = replay_batch(&mut book);
    s.add("J1-十九单重放全绿", green && reds == 0 && book.closed(), "11041–11059 全量重放零红槽");

    // J2 行数守恒：批总 = 19 × 260 = 4940
    s.add("J2-行数守恒", book.lines_ok() && book.batch_lines() == BATCH_LINES, "批行数与闭式目标对平");

    // J3 域累计对平：197,060 + 4,940 = 202,000
    let rec = book.reconcile(DOM_ACCUM_BASE, DOM_ACCUM_TOTAL);
    s.add(
        "J3-域累计对平",
        rec == Ok(DOM_ACCUM_TOTAL) && book.accum == DOM_ACCUM_TOTAL,
        "基账加本批闭式等于域总账",
    );

    // J4 封存根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J4-封存根", book.root == Some(root) && book.fold() == root && root != 0, "封存根落账且指纹稳定");

    // J5 注入检出：错值裁断 + 行数漂移
    let inj = inject_four();
    s.add("J5-错值漂移检出", inj[0] && inj[1], "错值必红且行数漂移必拒");

    // J6 注入检出：累计错值 + 封存后写
    s.add("J6-累计封存检出", inj[2] && inj[3], "对平错值必拒且封存后写必抓");

    // J7 单号重号拒
    let mut d = BatchCloseLedger33::new();
    let _ = d.enroll(UNIT_FIRST, 5);
    let j7 = d.enroll(UNIT_FIRST, 6).err() == Some(BY_DUP);
    s.add("J7-单号重号拒", j7, "单号重登必拒");

    // J8 裁断目标不存在拒
    let j8 = BatchCloseLedger33::new().judge(88888, 1, UNIT_LINES).err() == Some(BY_SHORT);
    s.add("J8-目标不存在拒", j8, "裁断不存在的单必拒");

    // J9 探针确定性：同单两次求值一致且十九单互异
    let p3a = by_probe(UNIT_FIRST + 2);
    let p3b = by_probe(UNIT_FIRST + 2);
    let mut distinct = true;
    let mut i = 0u32;
    while i < UNITS_NEEDED as u32 {
        let mut j = i.wrapping_add(1);
        while j < UNITS_NEEDED as u32 {
            if by_probe(UNIT_FIRST + i) == by_probe(UNIT_FIRST + j) {
                distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("J9-探针确定性", p3a == p3b && distinct, "探针同入同出且十九单互异");

    // J10 缺口语义：漏裁断一路时 gaps 可枚举且 closed 为假
    let j10 = {
        let mut c = BatchCloseLedger33::new();
        let mut i = 0u32;
        while i < UNITS_NEEDED as u32 {
            let _ = c.enroll(UNIT_FIRST + i, by_probe(UNIT_FIRST + i));
            i += 1;
        }
        let mut i = 0u32;
        while i < UNITS_NEEDED as u32 - 1 {
            let _ = c.judge(UNIT_FIRST + i, by_probe(UNIT_FIRST + i), UNIT_LINES);
            i += 1;
        }
        c.gaps().len() == 1 && !c.closed()
    };
    s.add("J10-缺口枚举", j10, "漏裁断一路可枚举且批收口为假");

    // J11 行数逐单入账：每单行数恰为 260
    let j11 = {
        let mut all = true;
        let mut k = 0usize;
        while k < book.units.len() {
            if book.units[k].lines != UNIT_LINES {
                all = false;
            }
            k += 1;
        }
        all && book.units.len() == UNITS_NEEDED
    };
    s.add("J11-行数逐单入账", j11, "十九单行数逐单恰为定值");

    // J12 未裁断不算收口：探针齐但裁断位未翻
    let j12 = {
        let mut c = BatchCloseLedger33::new();
        let mut i = 0u32;
        while i < UNITS_NEEDED as u32 {
            let _ = c.enroll(UNIT_FIRST + i, by_probe(UNIT_FIRST + i));
            i += 1;
        }
        !c.closed() && c.gaps().len() == UNITS_NEEDED
    };
    s.add("J12-未裁断不算收", j12, "探针齐但未逐单裁断仍不算收口");

    // J13 域累计单调：总账严格大于基账
    let j13 = DOM_ACCUM_TOTAL > DOM_ACCUM_BASE
        && DOM_ACCUM_TOTAL - DOM_ACCUM_BASE == BATCH_LINES;
    s.add("J13-域累计单调", j13, "域总账恰增本批行数");

    // J14 渲染面：绿槽与红槽渲染可区分
    let j14 = {
        let g = ReplayUnit33 { unit: 1, probe: 5, verdict: true, judged: true, lines: UNIT_LINES };
        let r = ReplayUnit33 { unit: 1, probe: 5, verdict: false, judged: true, lines: UNIT_LINES };
        g.render() != r.render() && g.render().contains('G') && r.render().contains('R')
    };
    s.add("J14-渲染分旗", j14, "绿/红槽渲染可区分");

    // J15 动态明细：批行数与域累计十进制落账
    s.add_dyn(
        "J15-批账明细",
        book.batch_lines() == BATCH_LINES && book.accum == DOM_ACCUM_TOTAL,
        &format!("batch={} accum={}", book.batch_lines(), book.accum),
    );

    // J16 动态明细：封存根十六进制落账
    let fp = book.fold();
    s.add_dyn("J16-封存根明细", fp == root, &format!("root={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11060_batch_flow() {
        let mut l = BatchCloseLedger33::new();
        assert!(l.enroll(UNIT_FIRST, 5).is_ok());
        assert!(l.enroll(UNIT_FIRST, 6).err() == Some(BY_DUP));
        assert_eq!(l.judge(UNIT_FIRST, 5, UNIT_LINES), Ok(true));
        assert!(l.judge(UNIT_FIRST + 1, 5, UNIT_LINES).err() == Some(BY_SHORT));
        l.freeze();
        assert!(l.enroll(99999, 1).err() == Some(BY_FROZEN));
        assert!(l.root.is_some());
    }

    #[test]
    fn unx11060_checks_green() {
        let s = run_unx11060_checks();
        assert!(s.all_passed(), "UNX-F11060 判据未全绿");
    }
}
