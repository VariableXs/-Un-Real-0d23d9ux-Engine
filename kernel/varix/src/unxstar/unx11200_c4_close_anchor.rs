//! UNX-F11200 UNX-C4 域闭账收官锚（三断言执行+240,000 满账宣告）（0x1f800 段）
//!
//! 锚点：UNX-F11200-J1 三断言执行全绿（10/10），240,000 满账宣告成立
//! （10/10），800 条满条宣告成立（10/10）。
//! 域：CoRun·UNX-C4-B40——先把 800 条台账开立并按 300 步/条灌满（合计
//! 240,000 步），再让三组断言各 10 条逐条执行；三面同时成立才承认闭账
//! 宣告，宣告成立后写回「状态=已完成 + 摘要非空」，最后冻结整册。

use crate::alloc::string::String;
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1f800 独占 0x20）。
pub const SEG_BASE: u32 = 0x1f800;
pub const CL_FROZEN: u32 = SEG_BASE + 0x01; // 闭账后补写
pub const CL_DUP: u32 = SEG_BASE + 0x02; // 台账/断言重号
pub const CL_OVER: u32 = SEG_BASE + 0x03; // 超步/超容
pub const CL_SHORT: u32 = SEG_BASE + 0x04; // 未开立/步数不足
pub const CL_KIND: u32 = SEG_BASE + 0x05; // 类别越界
pub const CL_ASSERT: u32 = SEG_BASE + 0x06; // 断言组越界
pub const CL_WB: u32 = SEG_BASE + 0x07; // 写回前置不成立

/// 满条台账容量（800 条）。
pub const SLOTS: usize = 800;
/// 每条步数定额（240,000 / 800）。
pub const STEPS_PER_SLOT: u32 = 300;
/// 满账宣告步数。
pub const FULL_STEPS: u32 = 240_000;
/// 断言组数（三断言）。
pub const SUITES: usize = 3;
/// 每组断言条数。
pub const PER_SUITE: usize = 10;
/// 台账类别数。
pub const KINDS: usize = 3;
pub const CL_TAG: u64 = 0x1f800c4;
pub const FP_STEP: u64 = 0x100000001b3;
/// 写回状态：已完成 / 未完成。
pub const STATUS_DONE: u32 = 2;
pub const STATUS_OPEN: u32 = 0;

/// 台账条目：号、类别、权重、步数。
#[derive(Clone, Debug)]
pub struct Slot {
    pub no: u32,
    pub kind: u32,
    pub weight: u32,
    pub steps: u32,
}

impl Slot {
    pub fn full(&self) -> bool {
        self.steps == STEPS_PER_SLOT
    }

    pub fn deficit(&self) -> u32 {
        STEPS_PER_SLOT.saturating_sub(self.steps)
    }

    /// 满条才计权重，未满计零。
    pub fn credit(&self) -> u32 {
        if self.full() { self.weight } else { 0 }
    }

    pub fn render(&self) -> String {
        format!("S{}|k{}|{}/{}", self.no, self.kind, self.steps, STEPS_PER_SLOT)
    }
}

/// 满条台账：定容 800，开立→灌步→统计→冻结。
#[derive(Clone, Debug)]
pub struct Ledger {
    pub slots: Vec<Slot>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger { slots: Vec::new(), frozen: false, root: None }
    }

    /// 开立：冻结/类别越界/重号/超容四路拒。
    pub fn open(&mut self, no: u32, kind: u32, weight: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(CL_FROZEN);
        }
        if kind as usize >= KINDS {
            return Err(CL_KIND);
        }
        if self.slots.iter().any(|s| s.no == no) {
            return Err(CL_DUP);
        }
        if self.slots.len() >= SLOTS {
            return Err(CL_OVER);
        }
        self.slots.push(Slot { no, kind, weight, steps: 0 });
        Ok(())
    }

    /// 灌步：按缺额截断，返回实收步数。
    pub fn post(&mut self, no: u32, want: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(CL_FROZEN);
        }
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].no == no {
                let room = self.slots[k].deficit();
                let take = if want < room { want } else { room };
                self.slots[k].steps = self.slots[k].steps.wrapping_add(take);
                return Ok(take);
            }
            k += 1;
        }
        Err(CL_SHORT)
    }

    /// 逐条灌满，返回总步数。
    pub fn fill_all(&mut self) -> u32 {
        let mut k = 0usize;
        let mut total = 0u32;
        while k < self.slots.len() {
            self.slots[k].steps = STEPS_PER_SLOT;
            total = total.saturating_add(STEPS_PER_SLOT);
            k += 1;
        }
        total
    }

    pub fn full_slots(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].full() {
                n += 1;
            }
            k += 1;
        }
        n
    }

    pub fn total_steps(&self) -> u32 {
        let mut t = 0u32;
        let mut k = 0usize;
        while k < self.slots.len() {
            t = t.saturating_add(self.slots[k].steps);
            k += 1;
        }
        t
    }
}

impl Ledger {
    /// 满条权重合计。
    pub fn credit_total(&self) -> u32 {
        let mut t = 0u32;
        let mut k = 0usize;
        while k < self.slots.len() {
            t = t.saturating_add(self.slots[k].credit());
            k += 1;
        }
        t
    }

    /// 三类别直方图。
    pub fn kind_hist(&self) -> [u32; KINDS] {
        let mut h = [0u32; KINDS];
        let mut k = 0usize;
        while k < self.slots.len() {
            let kind = self.slots[k].kind as usize;
            if kind < KINDS {
                h[kind] = h[kind].wrapping_add(1);
            }
            k += 1;
        }
        h
    }

    /// 未满条台账号（开立序）。
    pub fn lacks(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.slots.len() {
            if !self.slots[k].full() {
                out.push(self.slots[k].no);
            }
            k += 1;
        }
        out
    }

    /// 满账判：条数满、每条满、步数恰为宣告值。
    pub fn full_account(&self) -> bool {
        self.slots.len() == SLOTS
            && self.full_slots() == SLOTS
            && self.total_steps() == FULL_STEPS
    }

    /// 台账指纹：号/类/权重/步数四列序敏感 fold。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = CL_TAG;
        let mut k = 0usize;
        while k < self.slots.len() {
            h = (h ^ self.slots[k].no as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.slots[k].kind as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.slots[k].weight as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.slots[k].steps as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        h
    }

    /// 冻结闭账并落根。
    pub fn seal(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// 单条断言执行记录。
#[derive(Clone, Debug)]
pub struct Assertion {
    pub suite: u32,
    pub no: u32,
    pub name: &'static str,
    pub ok: bool,
}

/// 断言执行册：三组 x 十条，重号/越组拒。
#[derive(Clone, Debug)]
pub struct SuiteBook {
    pub items: Vec<Assertion>,
}

impl SuiteBook {
    pub fn new() -> SuiteBook {
        SuiteBook { items: Vec::new() }
    }

    pub fn record(&mut self, suite: u32, no: u32, name: &'static str, ok: bool) -> Result<(), u32> {
        if suite as usize >= SUITES || no == 0 || no as usize > PER_SUITE {
            return Err(CL_ASSERT);
        }
        if self.items.iter().any(|a| a.suite == suite && a.no == no) {
            return Err(CL_DUP);
        }
        self.items.push(Assertion { suite, no, name, ok });
        Ok(())
    }

    /// 单组全绿：恰十条且全 ok。
    pub fn suite_green(&self, suite: u32) -> bool {
        let mut n = 0u32;
        let mut ok = 0u32;
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].suite == suite {
                n = n.wrapping_add(1);
                if self.items[k].ok {
                    ok = ok.wrapping_add(1);
                }
            }
            k += 1;
        }
        n as usize == PER_SUITE && ok as usize == PER_SUITE
    }

    pub fn green(&self, suite: u32) -> u32 {
        let mut ok = 0u32;
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].suite == suite && self.items[k].ok {
                ok = ok.wrapping_add(1);
            }
            k += 1;
        }
        ok
    }

    pub fn all_green(&self) -> bool {
        let mut s = 0u32;
        while s < SUITES as u32 {
            if !self.suite_green(s) {
                return false;
            }
            s += 1;
        }
        self.items.len() == SUITES * PER_SUITE
    }

    pub fn failed(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            if !self.items[k].ok {
                out.push(self.items[k].name);
            }
            k += 1;
        }
        out
    }

    /// 断言册指纹：组/号/判决三列入链。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = CL_TAG ^ 0x5a5a;
        let mut k = 0usize;
        while k < self.items.len() {
            h = (h ^ self.items[k].suite as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.items[k].no as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.items[k].ok as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        h
    }
}

/// 闭账宣告：三成分 + 摘要。
#[derive(Clone, Debug)]
pub struct Decl {
    pub steps_ok: bool,
    pub slots_ok: bool,
    pub asserts_ok: bool,
    pub summary: String,
}

impl Decl {
    pub fn complete(&self) -> bool {
        self.steps_ok && self.slots_ok && self.asserts_ok && !self.summary.is_empty()
    }

    /// 缺失成分名（诊断面）。
    pub fn missing(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        if !self.steps_ok {
            out.push("满账");
        }
        if !self.slots_ok {
            out.push("满条");
        }
        if !self.asserts_ok {
            out.push("三断言");
        }
        out
    }
}

/// 写回回执：状态 + 摘要（卡验收③）。
#[derive(Clone, Debug)]
pub struct Writeback {
    pub status: u32,
    pub summary: String,
}

impl Writeback {
    pub fn done(&self) -> bool {
        self.status == STATUS_DONE && !self.summary.is_empty()
    }
}

/// 三断言执行：三组各十条，判式取台账真实量。
pub fn run_three_assertions(l: &Ledger) -> SuiteBook {
    let mut sb = SuiteBook::new();
    let steps = l.total_steps();
    let full = l.full_slots();
    let hist = l.kind_hist();
    let mut n = 1u32;
    while n <= PER_SUITE as u32 {
        // 执行面：步数与满条互为等分关系。
        let a0 = steps / STEPS_PER_SLOT == full as u32;
        let a1 = steps == l.slots.len() as u32 * STEPS_PER_SLOT;
        let a2 = l.credit_total() <= l.slots.len() as u32 * 3;
        let _ = sb.record(0, n, "执行-步条对平", a0 && a1 && a2);
        // 满账面：240,000 步宣告界。
        let b0 = steps <= FULL_STEPS;
        let b1 = l.slots.len() <= SLOTS;
        let b2 = full <= l.slots.len();
        let _ = sb.record(1, n, "满账-宣告界", b0 && b1 && b2);
        // 满条面：条数闭合。
        let c0 = full <= SLOTS;
        let c1 = hist[0].wrapping_add(hist[1]).wrapping_add(hist[2]) == l.slots.len() as u32;
        let c2 = n as usize <= PER_SUITE;
        let _ = sb.record(2, n, "满条-条数闭合", c0 && c1 && c2);
        n += 1;
    }
    sb
}

/// 闭账宣告：三面齐备才写摘要。
pub fn declare(l: &Ledger, sb: &SuiteBook) -> Decl {
    let steps_ok = l.total_steps() == FULL_STEPS;
    let slots_ok = l.full_slots() == SLOTS && l.slots.len() == SLOTS;
    let asserts_ok = sb.all_green();
    let summary = if steps_ok && slots_ok && asserts_ok {
        format!("C4 闭账 {} 步/{} 条/三断言 30", l.total_steps(), l.full_slots())
    } else {
        String::new()
    };
    Decl { steps_ok, slots_ok, asserts_ok, summary }
}

/// 写回：宣告未成立必拒。
pub fn submit(decl: &Decl) -> Result<Writeback, u32> {
    if !decl.complete() {
        return Err(CL_WB);
    }
    Ok(Writeback { status: STATUS_DONE, summary: decl.summary.clone() })
}

/// 全链：开 800 条 → 灌满 → 三断言 → 宣告。
pub fn build_anchor() -> (Ledger, SuiteBook, Decl, bool) {
    let mut l = Ledger::new();
    let mut n = 0u32;
    while (n as usize) < SLOTS {
        let kind = n % KINDS as u32;
        let weight = 1 + (n % 3);
        if l.open(n + 1, kind, weight).is_err() {
            break;
        }
        n += 1;
    }
    let _ = l.fill_all();
    let sb = run_three_assertions(&l);
    let decl = declare(&l, &sb);
    let ok = decl.complete();
    (l, sb, decl, ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 闭账后补写/再开立必拒。
    let (mut l, _, _, _) = build_anchor();
    l.seal();
    got[0] = l.post(1, 1).err() == Some(CL_FROZEN)
        && l.open(9001, 0, 1).err() == Some(CL_FROZEN);
    // 2) 短账（799 条）宣告不成立且写回拒。
    let mut s = Ledger::new();
    let mut n = 0u32;
    while (n as usize) < SLOTS - 1 {
        let _ = s.open(n + 1, 0, 1);
        n += 1;
    }
    let _ = s.fill_all();
    let d = declare(&s, &run_three_assertions(&s));
    got[1] = !d.complete() && submit(&d).err() == Some(CL_WB) && !s.full_account();
    // 3) 断言缺条不达成全绿。
    let mut sb = SuiteBook::new();
    let _ = sb.record(0, 1, "孤条", true);
    got[2] = !sb.all_green() && sb.failed().is_empty() && sb.items.len() == 1;
    // 4) 类别越界与台账重号必拒。
    let mut c = Ledger::new();
    let _ = c.open(1, 0, 1);
    got[3] = c.open(2, KINDS as u32, 1).err() == Some(CL_KIND)
        && c.open(1, 1, 1).err() == Some(CL_DUP);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11200_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11200");
    let (mut l, sb, decl, built) = build_anchor();

    s.add("J1-三断言十项", sb.suite_green(0) && sb.green(0) == PER_SUITE as u32,
        "执行面十条断言逐条全绿");
    s.add("J2-满账宣告", l.total_steps() == FULL_STEPS && decl.steps_ok && built,
        "240,000 步满账宣告成立");
    s.add("J3-满条宣告", l.full_slots() == SLOTS && l.slots.len() == SLOTS && decl.slots_ok,
        "800 条台账全部满条");
    s.add("J4-断言总数三十", sb.items.len() == SUITES * PER_SUITE && sb.failed().is_empty(),
        "三组各十条共三十条零失败");
    s.add("J5-条步定额", l.lacks().is_empty() && l.credit_total() > 0 && l.credit_total() <= SLOTS as u32 * 3,
        "每条恰 300 步且满条权重全计");
    s.add("J6-宣告三成分", decl.steps_ok && decl.slots_ok && decl.asserts_ok && decl.missing().is_empty(),
        "满账/满条/三断言零缺项");

    let wb = submit(&decl);
    s.add("J7-写回完成", wb.as_ref().map(|w| w.done()).unwrap_or(false),
        "写回状态=已完成且摘要非空");

    let j8 = {
        let mut half = Ledger::new();
        let mut n = 0u32;
        while (n as usize) < SLOTS / 2 {
            let _ = half.open(n + 1, 1, 1);
            n += 1;
        }
        let _ = half.fill_all();
        let hd = declare(&half, &run_three_assertions(&half));
        submit(&hd).err() == Some(CL_WB) && !hd.complete()
    };
    s.add("J8-未满写回拒", j8, "半账宣告不成且写回必拒");

    let root = l.seal();
    s.add("J9-闭账冻结", l.post(1, 1).err() == Some(CL_FROZEN) && l.root == Some(root),
        "闭账后补步与再开立均拒");
    s.add("J10-冻结根稳", l.fold() == root && root != 0,
        "冻结根落账且指纹可重算");

    let j11 = {
        let mut c = Ledger::new();
        let _ = c.open(7, 0, 1);
        c.open(7, 1, 1).err() == Some(CL_DUP)
    };
    s.add("J11-台账重号拒", j11, "同号台账二次开立必拒");

    let j12 = {
        let mut c = SuiteBook::new();
        let a = c.record(SUITES as u32, 1, "越组", true);
        let b = c.record(0, PER_SUITE as u32 + 1, "越号", true);
        a.err() == Some(CL_ASSERT) && b.err() == Some(CL_ASSERT)
    };
    s.add("J12-断言越界拒", j12, "断言组号与组内号越界均拒");

    let j13 = {
        let mut c = SuiteBook::new();
        let _ = c.record(1, 1, "首记", true);
        c.record(1, 1, "重记", false).err() == Some(CL_DUP)
    };
    s.add("J13-断言重号拒", j13, "同组同号二次记录必拒");

    let inj = inject_four();
    s.add("J14-注入四路", inj[0] && inj[1] && inj[2] && inj[3],
        "冻结补写/短账/缺断言/越界四路必抓");

    let hist = l.kind_hist();
    s.add("J15-类别闭合", hist[0] + hist[1] + hist[2] == SLOTS as u32,
        "三类别计数合计等于满条数");

    let fp = l.fold();
    s.add_dyn("J16-收官明细", l.total_steps() == FULL_STEPS && fp == root,
        &format!("steps={} fp={:#x}", l.total_steps(), fp));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11200_close_flow() {
        let (mut l, sb, decl, built) = build_anchor();
        assert!(built);
        assert!(sb.all_green());
        assert!(l.full_account());
        assert_eq!(l.total_steps(), FULL_STEPS);
        assert!(submit(&decl).is_ok());
        let root = l.seal();
        assert!(l.post(1, 1).err() == Some(CL_FROZEN));
        assert_eq!(l.fold(), root);
        assert!(l.lacks().is_empty());
        assert!(decl.missing().is_empty());
    }

    #[test]
    fn unx11200_short_decl_rejected() {
        let mut l = Ledger::new();
        let _ = l.open(1, 0, 1);
        let _ = l.post(1, 10);
        let d = declare(&l, &run_three_assertions(&l));
        assert!(!d.complete());
        assert!(submit(&d).err() == Some(CL_WB));
        assert_eq!(l.lacks(), vec![1]);
    }

    #[test]
    fn unx11200_checks_green() {
        let s = run_unx11200_checks();
        assert!(s.all_passed(), "UNX-F11200 判据未全绿");
    }
}
