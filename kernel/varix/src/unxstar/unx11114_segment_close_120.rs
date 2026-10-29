//! UNX-F11114 段级收官账（B31–B36 段 120 条断言）（0x1f594 段）
//!
//! B31–B36 六批共 120 条断言必须全部落在段账内：一条一号、一批二十条、
//! 批内编号连续、零红。本模块以段账 `SegmentLedger` 记账，`declare` 对平
//! 条数与批数，`seal` 固化后拒绝一切写入。

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址：F11114 独占 0x1f594。
pub const SEG_BASE: u32 = 0x1f594;

/// 断言号越界（合法域 1..=120）。
pub const SG_RANGE: u32 = SEG_BASE + 0x01;
/// 批号越出 B31–B36。
pub const SG_BATCH: u32 = SEG_BASE + 0x02;
/// 断言号重复登记。
pub const SG_DUP: u32 = SEG_BASE + 0x03;
/// 段内有红断言，不允许收官。
pub const SG_RED: u32 = SEG_BASE + 0x04;
/// 条数不足 120，段账偏薄。
pub const SG_THIN: u32 = SEG_BASE + 0x05;
/// 声明条数与批分布对不平。
pub const SG_UNBAL: u32 = SEG_BASE + 0x06;
/// 段账已冻结，拒绝写入。
pub const SG_FROZEN: u32 = SEG_BASE + 0x07;
/// 登记容量溢出。
pub const SG_OVER: u32 = SEG_BASE + 0x08;

/// 段内断言总数：6 批 × 20 条。
pub const TOTAL: usize = 120;
/// 每批断言数。
pub const PER_BATCH: usize = 20;
/// 段首批号。
pub const BATCH_FIRST: usize = 31;
/// 段末批号。
pub const BATCH_LAST: usize = 36;
/// 注册容量上限。
pub const CAP: usize = 124;
/// 折叠种子标签。
const SG_TAG: u64 = 0x0000_1f5_9494;
/// FNV-1a 素数。
const FP_STEP: u64 = 0x100000001b3;

/// 单条段内断言。
#[derive(Clone, Debug)]
pub struct SegAssert {
    /// 全段连续断言号（1..=120）。
    pub no: usize,
    /// 所属批号（31..=36）。
    pub batch: usize,
    /// 是否通过。
    pub pass: bool,
}

impl SegAssert {
    /// 归一化渲染：`A{no}|B{batch}|P` 或 `A{no}|B{batch}|F`。
    pub fn render(&self) -> String {
        let flag = if self.pass { "P" } else { "F" };
        format!("A{}|B{}|{}", self.no, self.batch, flag)
    }
}

/// 段级收官账。
#[derive(Clone, Debug)]
pub struct SegmentLedger {
    items: Vec<SegAssert>,
    declared: usize,
    frozen: bool,
    root: u64,
}

impl SegmentLedger {
    /// 建空账。
    pub fn new() -> Self {
        Self { items: Vec::new(), declared: 0, frozen: false, root: 0 }
    }

    /// 登记一条断言：号域、批域、重号、容量四道闸。
    pub fn file(&mut self, no: usize, batch: usize, pass: bool) -> Result<(), u32> {
        if self.frozen {
            return Err(SG_FROZEN);
        }
        if no == 0 || no > TOTAL {
            return Err(SG_RANGE);
        }
        if batch < BATCH_FIRST || batch > BATCH_LAST {
            return Err(SG_BATCH);
        }
        if self.items.len() >= CAP {
            return Err(SG_OVER);
        }
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].no == no {
                return Err(SG_DUP);
            }
            k = k.wrapping_add(1);
        }
        self.items.push(SegAssert { no, batch, pass });
        Ok(())
    }

    /// 本批已登记条数。
    pub fn in_batch(&self, batch: usize) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].batch == batch {
                n = n.wrapping_add(1);
            }
            k = k.wrapping_add(1);
        }
        n
    }

    /// 已登记条数。
    pub fn count(&self) -> usize {
        self.items.len()
    }

    /// 覆盖到的批数。
    pub fn batch_count(&self) -> usize {
        let mut n = 0usize;
        let mut b = BATCH_FIRST;
        while b <= BATCH_LAST {
            if self.in_batch(b) > 0 {
                n = n.wrapping_add(1);
            }
            b = b.wrapping_add(1);
        }
        n
    }

    /// 红断言号清单（升序，最多 16 条）。
    pub fn reds(&self) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            if !self.items[k].pass && out.len() < 16 {
                out.push(self.items[k].no);
            }
            k = k.wrapping_add(1);
        }
        out.sort_by_key(|n| *n);
        out
    }

    /// 六批是否各满 20 条。
    pub fn batches_full(&self) -> bool {
        let mut b = BATCH_FIRST;
        while b <= BATCH_LAST {
            if self.in_batch(b) != PER_BATCH {
                return false;
            }
            b = b.wrapping_add(1);
        }
        true
    }

    /// 段账收官：条数对平、零红、批分布平整。
    pub fn declare(&mut self, total: usize) -> Result<(), u32> {
        if self.frozen {
            return Err(SG_FROZEN);
        }
        if self.items.len() < TOTAL {
            return Err(SG_THIN);
        }
        if self.items.len() != total || self.batch_count() != 6 {
            return Err(SG_UNBAL);
        }
        if !self.batches_full() {
            return Err(SG_UNBAL);
        }
        if !self.reds().is_empty() {
            return Err(SG_RED);
        }
        self.declared = total;
        Ok(())
    }

    /// 段账是否已收官。
    pub fn closed(&self) -> bool {
        self.declared == TOTAL && self.reds().is_empty() && self.batches_full()
    }

    /// 全段冻结：返回折叠根。
    pub fn seal(&mut self) -> u64 {
        self.frozen = true;
        self.root = self.fold();
        self.root
    }

    /// 已冻结否。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// FNV-1a 折叠。
    pub fn fold(&self) -> u64 {
        let mut h = SG_TAG;
        let mut k = 0usize;
        while k < self.items.len() {
            let it = &self.items[k];
            h = (h ^ it.no as u64).wrapping_mul(FP_STEP);
            h = (h ^ it.batch as u64).wrapping_mul(FP_STEP);
            let bit = if it.pass { 1u64 } else { 2u64 };
            h = (h ^ bit).wrapping_mul(FP_STEP);
            k = k.wrapping_add(1);
        }
        h = (h ^ self.declared as u64).wrapping_mul(FP_STEP);
        h
    }
}

/// 断言号 → 批号（1..=120 → 31..=36）。
pub fn batch_of(no: usize) -> usize {
    BATCH_FIRST + (no.saturating_sub(1)) / PER_BATCH
}

/// 标准段账：1..=120 全绿并收官。
pub fn build_ledger() -> SegmentLedger {
    let mut led = SegmentLedger::new();
    let mut no = 1usize;
    while no <= TOTAL {
        let _ = led.file(no, batch_of(no), true);
        no = no.wrapping_add(1);
    }
    let _ = led.declare(TOTAL);
    led
}

/// 四路对抗：越界号、越段批、重号、单红断言。
pub fn inject_four() -> [bool; 4] {
    let mut led = SegmentLedger::new();
    let a = led.file(0, 31, true) == Err(SG_RANGE);
    let b = led.file(1, 37, true) == Err(SG_BATCH);
    let _ = led.file(5, 31, true);
    let c = led.file(5, 31, true) == Err(SG_DUP);
    let mut dirty = SegmentLedger::new();
    let mut no = 1usize;
    while no <= TOTAL {
        let pass = no != 77;
        let _ = dirty.file(no, batch_of(no), pass);
        no = no.wrapping_add(1);
    }
    let d = dirty.declare(TOTAL) == Err(SG_RED) && dirty.reds() == [77];
    [a, b, c, d]
}

/// F11114 全部检查：段账 120 条断言对平与四路对抗。
pub fn run_unx11114_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11114 段级收官账");
    let led = build_ledger();
    let four = inject_four();

    // J1 段账三数对平：120 条 / 6 批 / 每批 20。
    s.add(
        "J1-三数对平",
        led.count() == TOTAL && led.batch_count() == 6 && PER_BATCH * 6 == TOTAL,
        "段账 120 条、6 批、每批 20 条，三数必须互相对平",
    );
    // J2 六批账闭合：每批恰好 20 条且已声明。
    s.add(
        "J2-六批闭合",
        led.batches_full() && led.closed() && led.in_batch(31) == 20 && led.in_batch(36) == 20,
        "B31 至 B36 六批各 20 条，段账声明后闭合",
    );
    // J3 零红：段内无失败断言。
    s.add("J3-零红", led.reds().is_empty(), "段账 120 条全绿，红断言清单为空");
    // J4 越界断言号被拒。
    let mut probe = SegmentLedger::new();
    s.add(
        "J4-号域闸",
        probe.file(121, 36, true) == Err(SG_RANGE) && probe.file(0, 31, true) == Err(SG_RANGE),
        "断言号 0 与 121 均越出 1..=120，登记必须报 SG_RANGE",
    );
    // J5 越段批号被拒。
    s.add(
        "J5-批域闸",
        probe.file(1, 30, true) == Err(SG_BATCH) && probe.file(1, 37, true) == Err(SG_BATCH),
        "批号 30 与 37 越出 B31–B36，登记必须报 SG_BATCH",
    );
    // J6 重号被拒。
    let mut dup = SegmentLedger::new();
    let _ = dup.file(9, 31, true);
    s.add(
        "J6-重号闸",
        dup.file(9, 31, true) == Err(SG_DUP) && dup.count() == 1,
        "同一断言号二次登记必须报 SG_DUP 且不增账",
    );
    // J7 条数不足被拒。
    let mut thin = SegmentLedger::new();
    let mut no = 1usize;
    while no < TOTAL {
        let _ = thin.file(no, batch_of(no), true);
        no = no.wrapping_add(1);
    }
    s.add(
        "J7-偏薄闸",
        thin.declare(119) == Err(SG_THIN) && !thin.closed(),
        "只登记 119 条时收官必须报 SG_THIN",
    );
    // J8 批分布不平被拒。
    let mut flat = SegmentLedger::new();
    let mut k = 1usize;
    while k <= TOTAL {
        let _ = flat.file(k, 31, true);
        k = k.wrapping_add(1);
    }
    s.add(
        "J8-分布闸",
        flat.declare(TOTAL) == Err(SG_UNBAL) && flat.batch_count() == 1,
        "120 条全挤在 B31 时批数只有 1，收官必须报 SG_UNBAL",
    );
    // J9 对抗一：号域与批域两道闸同时有效。
    s.add("J9-对抗号批", four[0] && four[1], "inject_four 前两路：越界号与越段批均被拒");
    // J10 对抗二：重号与单红断言同时有效。
    s.add(
        "J10-对抗重红",
        four[2] && four[3],
        "inject_four 后两路：重号被拒、单红断言使收官报 SG_RED",
    );
    // J11 批内计数准确。
    s.add(
        "J11-批内计数",
        led.in_batch(33) == PER_BATCH && led.in_batch(30) == 0 && led.in_batch(37) == 0,
        "B33 恰 20 条，段外批号 30 与 37 的计数为 0",
    );
    // J12 冻结后拒绝写入。
    let mut sealed = build_ledger();
    let root = sealed.seal();
    s.add(
        "J12-冻结拒写",
        sealed.is_frozen() && sealed.file(1, 31, true) == Err(SG_FROZEN) && root != 0,
        "seal 后段账冻结，任何登记均报 SG_FROZEN",
    );
    // J13 冻结根稳定。
    s.add(
        "J13-冻结根",
        sealed.fold() == root && root != SG_TAG,
        "冻结根等于冻结时刻的折叠值，且不等于种子",
    );
    // J14 渲染格式。
    let sample = SegAssert { no: 120, batch: 36, pass: true };
    s.add(
        "J14-渲染",
        sample.render() == "A120|B36|P",
        "末条断言渲染为 A120|B36|P",
    );
    // J15 动态明细。
    s.add_dyn(
        "J15-段账明细",
        led.count() == 120,
        &format!(
            "段账条数={} 批数={} 每批={} 红={}",
            led.count(),
            led.batch_count(),
            PER_BATCH,
            led.reds().len()
        ),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11114_segment_flow() {
        let mut led = build_ledger();
        assert_eq!(led.count(), TOTAL);
        assert_eq!(led.batch_count(), 6);
        assert!(led.closed());
        let root = led.seal();
        assert_eq!(led.fold(), root);
        assert_eq!(led.file(1, 31, true), Err(SG_FROZEN));
        assert_eq!(batch_of(1), 31);
        assert_eq!(batch_of(TOTAL), 36);
    }

    #[test]
    fn unx11114_checks_green() {
        let s = run_unx11114_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
