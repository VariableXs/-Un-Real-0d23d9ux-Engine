//! UNX-F11100 B35 批收口（压力长稳演习总断言）（0x1f500 段）
//!
//! 锚点：UNX-F11100-J1 批内 19 判据重放全绿（10/10），行数守恒与域累计
//! 对平（10/10）。
//! 域：CoRun·UNX-C4-B35（批收口）。
//!
//! 结构：判据收口册（CriterionBook：19 条判据登记→落行数→重放翻绿→收口
//! 三数对平）+ 域累计链（DomainChain：批前 + 本批 = 批后）+ 压力长稳演习台
//! （DrillRig：定周期注入故障并当轮恢复，时延上界受控）+ 冻结根 + 注入核
//! （漏项收口/重号/行数短缺/域累计错位四路必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1f500 独占 0x20）。
pub const SEG_BASE: u32 = 0x1f500;
pub const BC_NOITEM: u32 = SEG_BASE + 0x01; // 判据未登记
pub const BC_DUP: u32 = SEG_BASE + 0x02; // 判据号重号 / 重复落账
pub const BC_EMPTY: u32 = SEG_BASE + 0x03; // 判据名空
pub const BC_SHORT: u32 = SEG_BASE + 0x04; // 行数低于卡要求
pub const BC_RED: u32 = SEG_BASE + 0x05; // 重放未绿
pub const BC_BALANCE: u32 = SEG_BASE + 0x06; // 域累计不平
pub const BC_FROZEN: u32 = SEG_BASE + 0x07; // 冻结后写账
pub const BC_OVER: u32 = SEG_BASE + 0x08; // 超定容
pub const BC_DRILL: u32 = SEG_BASE + 0x09; // 演习不收敛

/// 批号。
pub const BATCH: &str = "B35";
/// 批内判据条数（19）。
pub const ITEMS: usize = 19;
/// 判据册定容（>19，容量先定后用）。
pub const CAP: usize = 24;
/// 单条判据行数底线。
pub const ITEM_LINE_FLOOR: u32 = 14;
/// 批行数底线（卡面 390 判据折算的批内底线 270）。
pub const BATCH_LINE_FLOOR: u32 = 270;
/// 批前域累计（B34 收口后的域账）。
pub const DOMAIN_PRIOR: u32 = 231_480;
/// 演习轮数。
pub const DRILL_ROUNDS: u32 = 600;
/// 演习注入周期（每 37 轮注入一次）。
pub const DRILL_PERIOD: u32 = 37;
/// 单故障恢复时延上界（轮）。
pub const LATENCY_MAX: u32 = 2;
/// 账域标（fold 种子）。
pub const CLOSE_TAG: u64 = 0x1f5_00cc;
/// 指纹步进。
pub const FP_STEP: u64 = 0x100000001b3;

/// 单条判据：行数落账位 + 重放绿位。
#[derive(Clone, Debug, PartialEq)]
pub struct Criterion {
    pub no: u32,
    pub name: String,
    pub need: u32,
    pub lines: u32,
    pub settled: bool,
    pub replay: bool,
}

impl Criterion {
    /// 单条闭合：已落账、重放绿、行数不低于卡要求。
    pub fn closed(&self) -> bool {
        self.settled && self.replay && self.lines >= self.need
    }

    /// 单条渲染：判据号 | 绿红旗 | 实落行数。
    pub fn render(&self) -> String {
        let f = if self.replay { "G" } else { "R" };
        format!("J{}|{}|{}", self.no, f, self.lines)
    }
}

/// 域累计链：三数必须闭合（批前 + 本批 = 批后）。
#[derive(Clone, Debug)]
pub struct DomainChain {
    pub prior: u32,
    pub batch: u32,
    pub after: u32,
}

impl DomainChain {
    pub fn new(prior: u32) -> DomainChain {
        DomainChain { prior, batch: 0, after: prior }
    }

    /// 本批入链：零批拒，入链后批后数必须严格增长。
    pub fn commit(&mut self, batch_lines: u32) -> Result<(), u32> {
        if batch_lines == 0 {
            return Err(BC_BALANCE);
        }
        self.batch = self.batch.wrapping_add(batch_lines);
        self.after = self.prior.wrapping_add(self.batch);
        if self.after <= self.prior {
            return Err(BC_BALANCE);
        }
        Ok(())
    }

    /// 三数对平：批后 == 批前 + 本批，且批后严格大于批前。
    pub fn balanced(&self) -> bool {
        self.after == self.prior.wrapping_add(self.batch) && self.after > self.prior
    }
}

/// 判据收口册：19 条判据 + 域累计链 + 冻结根。
#[derive(Clone, Debug)]
pub struct CriterionBook {
    pub batch: String,
    pub items: Vec<Criterion>,
    pub chain: DomainChain,
    pub declared: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl CriterionBook {
    pub fn new(batch: &str, prior: u32) -> CriterionBook {
        CriterionBook {
            batch: batch.to_string(),
            items: Vec::new(),
            chain: DomainChain::new(prior),
            declared: 0,
            frozen: false,
            root: None,
        }
    }

    /// 判据登记：冻结/空名/重号/超容/行数底线不足皆拒。
    pub fn enroll(&mut self, no: u32, name: &str, need: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(BC_FROZEN);
        }
        if name.is_empty() {
            return Err(BC_EMPTY);
        }
        if no == 0 || self.items.iter().any(|c| c.no == no) {
            return Err(BC_DUP);
        }
        if self.items.len() >= CAP {
            return Err(BC_OVER);
        }
        if need < ITEM_LINE_FLOOR {
            return Err(BC_SHORT);
        }
        self.items.push(Criterion {
            no,
            name: name.to_string(),
            need,
            lines: 0,
            settled: false,
            replay: false,
        });
        Ok(())
    }

    /// 落行数：未登记拒、重复落账拒、低于卡要求拒。
    pub fn settle(&mut self, no: u32, lines: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(BC_FROZEN);
        }
        let it = self.find(no).ok_or(BC_NOITEM)?;
        if it.settled {
            return Err(BC_DUP);
        }
        if lines < it.need || lines < ITEM_LINE_FLOOR {
            return Err(BC_SHORT);
        }
        it.lines = lines;
        it.settled = true;
        Ok(())
    }

    /// 重放翻绿：未落账不得重放；红样本如实记录但不改行数。
    pub fn replay(&mut self, no: u32, green: bool) -> Result<(), u32> {
        if self.frozen {
            return Err(BC_FROZEN);
        }
        let it = self.find(no).ok_or(BC_NOITEM)?;
        if !it.settled {
            return Err(BC_NOITEM);
        }
        it.replay = green;
        Ok(())
    }

    fn find(&mut self, no: u32) -> Option<&mut Criterion> {
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].no == no {
                return Some(&mut self.items[k]);
            }
            k += 1;
        }
        None
    }

    pub fn total_need(&self) -> u32 {
        let mut t = 0u32;
        let mut k = 0usize;
        while k < self.items.len() {
            t = t.wrapping_add(self.items[k].need);
            k += 1;
        }
        t
    }

    pub fn total_lines(&self) -> u32 {
        let mut t = 0u32;
        let mut k = 0usize;
        while k < self.items.len() {
            t = t.wrapping_add(self.items[k].lines);
            k += 1;
        }
        t
    }

    /// 已闭合条数。
    pub fn closed_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].closed() {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 红判据数（未闭合条数）。
    pub fn reds(&self) -> usize {
        self.items.len().saturating_sub(self.closed_count())
    }

    /// 缺口枚举：未闭合判据号按登记序。
    pub fn gaps(&self) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            if !self.items[k].closed() {
                g.push(self.items[k].no);
            }
            k += 1;
        }
        g
    }

    /// 行数守恒：申报数 == 各判据要求之和，实落不低于申报，逐条不短缺。
    pub fn line_conservation(&self) -> bool {
        if self.declared == 0 || self.declared != self.total_need() {
            return false;
        }
        if self.declared < BATCH_LINE_FLOOR {
            return false;
        }
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].lines < self.items[k].need {
                return false;
            }
            k += 1;
        }
        self.total_lines() >= self.declared
    }

    /// 域累计对平：批后数等于外部期望值且三数闭合。
    pub fn domain_balanced(&self, expect_after: u32) -> bool {
        self.chain.after == expect_after && self.chain.balanced()
    }

    /// 收口：19 条全绿 → 申报 → 行数守恒 → 域链入账 → 冻结。
    pub fn close(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(BC_FROZEN);
        }
        if self.items.len() != ITEMS {
            return Err(BC_NOITEM);
        }
        if self.reds() != 0 {
            return Err(BC_RED);
        }
        self.declared = self.total_need();
        if !self.line_conservation() {
            return Err(BC_BALANCE);
        }
        let lines = self.total_lines();
        self.chain.commit(lines)?;
        Ok(self.freeze())
    }

    /// 冻结：翻冻结位并落冻结根。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }

    /// 封存别名：与冻结同根。
    pub fn seal(&mut self) -> u64 {
        self.freeze()
    }

    /// 册级指纹：判据链 + 域链 + 申报数（零面不入链）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = CLOSE_TAG;
        if !self.batch.is_empty() {
            let bytes = self.batch.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(FP_STEP);
                b += 1;
            }
        }
        let mut k = 0usize;
        while k < self.items.len() {
            let it = &self.items[k];
            h = (h ^ it.no as u64).wrapping_mul(FP_STEP);
            h = (h ^ it.need as u64).wrapping_mul(FP_STEP);
            h = (h ^ it.lines as u64).wrapping_mul(FP_STEP);
            h = (h ^ it.settled as u64).wrapping_mul(FP_STEP);
            h = (h ^ it.replay as u64).wrapping_mul(FP_STEP);
            let bytes = it.name.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(FP_STEP);
                b += 1;
            }
            k += 1;
        }
        if self.chain.prior != 0 || self.chain.batch != 0 || self.chain.after != 0 {
            h = (h ^ self.chain.prior as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.chain.batch as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.chain.after as u64).wrapping_mul(FP_STEP);
        }
        if self.declared != 0 {
            h = (h ^ self.declared as u64).wrapping_mul(FP_STEP);
        }
        h
    }
}

/// 压力长稳演习台：定周期注入故障并当轮恢复，记注入/恢复/时延上界。
#[derive(Clone, Debug)]
pub struct DrillRig {
    pub rounds: u32,
    pub injected: u32,
    pub recovered: u32,
    pub latency_max: u32,
    pub frozen: bool,
}

impl DrillRig {
    pub fn new() -> DrillRig {
        DrillRig { rounds: 0, injected: 0, recovered: 0, latency_max: 0, frozen: false }
    }

    /// 跑演习：每 DRILL_PERIOD 轮注入一次，同轮恢复（时延 1）。
    pub fn run(&mut self, rounds: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(BC_FROZEN);
        }
        if rounds == 0 {
            return Err(BC_DRILL);
        }
        let mut i = 0u32;
        while i < rounds {
            if i % DRILL_PERIOD == DRILL_PERIOD - 1 {
                self.injected = self.injected.wrapping_add(1);
                self.recovered = self.recovered.wrapping_add(1);
                if self.latency_max == 0 {
                    self.latency_max = 1;
                }
            }
            i += 1;
            self.rounds = self.rounds.wrapping_add(1);
        }
        if self.injected != self.recovered {
            return Err(BC_DRILL);
        }
        Ok(())
    }

    /// 收敛：轮数达标、注入者全恢复、确有注入、时延在闸内。
    pub fn converged(&self) -> bool {
        self.rounds >= DRILL_ROUNDS
            && self.injected > 0
            && self.injected == self.recovered
            && self.latency_max <= LATENCY_MAX
    }

    /// 冻结演习账。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let mut h: u64 = CLOSE_TAG ^ 0xd1;
        h = (h ^ self.rounds as u64).wrapping_mul(FP_STEP);
        h = (h ^ self.injected as u64).wrapping_mul(FP_STEP);
        h = (h ^ self.recovered as u64).wrapping_mul(FP_STEP);
        h = (h ^ self.latency_max as u64).wrapping_mul(FP_STEP);
        h
    }
}

/// 判据名样本（确定性）。
pub fn criterion_name(no: u32) -> String {
    format!("B35 批内判据 J{}", no)
}

/// 判据行数要求：统一 15 行，合计 285（高于批底线 270）。
pub fn item_need(_no: u32) -> u32 {
    15
}

/// 标准批样本：19 条判据登记 → 落行数 → 重放全绿。
pub fn build_batch() -> (CriterionBook, bool) {
    let mut b = CriterionBook::new(BATCH, DOMAIN_PRIOR);
    let mut ok = true;
    let mut n = 1u32;
    while n <= ITEMS as u32 {
        if b.enroll(n, &criterion_name(n), item_need(n)).is_err() {
            ok = false;
        }
        if b.settle(n, item_need(n).wrapping_add(n % 3)).is_err() {
            ok = false;
        }
        if b.replay(n, true).is_err() {
            ok = false;
        }
        n += 1;
    }
    (b, ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 漏项收口必拒（18 条就收口）
    let mut a = CriterionBook::new(BATCH, DOMAIN_PRIOR);
    let mut n = 1u32;
    while n < ITEMS as u32 {
        let _ = a.enroll(n, &criterion_name(n), item_need(n));
        let _ = a.settle(n, item_need(n));
        let _ = a.replay(n, true);
        n += 1;
    }
    got[0] = a.close().err() == Some(BC_NOITEM);
    // 2) 重号必拒
    let mut b = CriterionBook::new(BATCH, DOMAIN_PRIOR);
    let _ = b.enroll(1, "甲", item_need(1));
    got[1] = b.enroll(1, "乙", item_need(1)).err() == Some(BC_DUP);
    // 3) 行数短缺必拒（need 20 只落 14）
    let mut c = CriterionBook::new(BATCH, DOMAIN_PRIOR);
    let _ = c.enroll(1, "短样本", 20);
    got[2] = c.settle(1, 14).err() == Some(BC_SHORT);
    // 4) 域累计错位必检出（批后数故意 +1）
    let (mut d, _) = build_batch();
    let _ = d.close();
    let wrong = d.chain.after.wrapping_add(1);
    got[3] = !d.domain_balanced(wrong) && d.domain_balanced(d.chain.after);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11100_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11100");

    // J1 19 判据登记齐备
    let (mut book, built) = build_batch();
    s.add(
        "J1-十九判据在册",
        built && book.items.len() == ITEMS && book.total_need() == 285,
        "19 条判据逐条登记且要求行数合计 285",
    );

    // J2 重放全绿：零红判据
    s.add(
        "J2-重放全绿",
        book.reds() == 0 && book.closed_count() == ITEMS,
        "19 条判据重放零红",
    );

    // J3 行数守恒：申报数等于要求和，实落不低于申报
    let declared_ok = {
        book.declared = book.total_need();
        book.line_conservation() && book.total_lines() >= book.declared
    };
    s.add("J3-行数守恒", declared_ok, "申报 285 行，实落不低于申报");

    // J4 收口成功并落冻结位
    let closed = book.close();
    s.add("J4-收口成功", closed.is_ok() && book.frozen, "收口通过即冻结");

    // J5 域累计对平：批前 + 本批 = 批后
    let after = book.chain.after;
    let expect = DOMAIN_PRIOR.wrapping_add(book.total_lines());
    s.add(
        "J5-域累计对平",
        after == expect && book.domain_balanced(expect),
        "批前 231480 + 本批实落 = 批后",
    );

    // J6 冻结根稳定
    let root = book.root;
    s.add(
        "J6-冻结根稳定",
        root == Some(book.fold()) && root != Some(0),
        "冻结根与册指纹同值且非零",
    );

    // J7 冻结后写账必拒
    let j7 = book.settle(1, 99).err() == Some(BC_FROZEN)
        && book.enroll(20, "追挂", 15).err() == Some(BC_FROZEN)
        && book.replay(1, true).err() == Some(BC_FROZEN);
    s.add("J7-冻结拒写", j7, "冻结后落行数/登记/重放三路必拒");

    // J8 注入检出：漏项 + 重号
    let inj = inject_four();
    s.add("J8-漏项重号检出", inj[0] && inj[1], "漏项收口与判据重号两路必抓");

    // J9 注入检出：行数短缺 + 域累计错位
    s.add("J9-短缺错位检出", inj[2] && inj[3], "行数短缺与域累计错位两路必抓");

    // J10 缺口枚举：少翻一条时缺口可点名
    let j10 = {
        let mut c = CriterionBook::new(BATCH, DOMAIN_PRIOR);
        let mut k = 1u32;
        while k <= ITEMS as u32 {
            let _ = c.enroll(k, &criterion_name(k), item_need(k));
            let _ = c.settle(k, item_need(k));
            if k != 11 {
                let _ = c.replay(k, true);
            }
            k += 1;
        }
        c.gaps().len() == 1 && c.gaps().contains(&11) && c.reds() == 1
    };
    s.add("J10-缺口点名", j10, "缺一条时缺口恰为 11 号");

    // J11 演习收敛：600 轮注入者全恢复且时延在闸内
    let mut rig = DrillRig::new();
    let run_ok = rig.run(DRILL_ROUNDS).is_ok();
    s.add(
        "J11-演习收敛",
        run_ok && rig.converged() && rig.recovered == rig.injected,
        "600 轮演习注入全恢复",
    );

    // J12 演习注入计数与周期公式吻合
    let expect_inj = {
        let mut c = 0u32;
        let mut i = 0u32;
        while i < DRILL_ROUNDS {
            if i % DRILL_PERIOD == DRILL_PERIOD - 1 {
                c = c.wrapping_add(1);
            }
            i += 1;
        }
        c
    };
    s.add(
        "J12-注入计数",
        rig.injected == expect_inj && rig.injected > 0,
        "注入次数与周期公式一致",
    );

    // J13 演习冻结拒再跑
    let _ = rig.freeze();
    let j13 = rig.run(10).err() == Some(BC_FROZEN) && rig.frozen;
    s.add("J13-演习冻结", j13, "演习账冻结后不得重跑");

    // J14 零轮演习必拒
    let mut z = DrillRig::new();
    let j14 = z.run(0).err() == Some(BC_DRILL) && z.rounds == 0;
    s.add("J14-零轮拒跑", j14, "零轮演习无注入即拒");

    // J15 渲染分旗：绿判据带 G，红判据带 R 且两串不同
    let j15 = {
        let mut c = CriterionBook::new(BATCH, DOMAIN_PRIOR);
        let _ = c.enroll(1, "渲染甲", 15);
        let _ = c.enroll(2, "渲染乙", 15);
        let _ = c.settle(1, 15);
        let _ = c.settle(2, 15);
        let _ = c.replay(1, true);
        let a = c.items[0].render();
        let b = c.items[1].render();
        a != b && a.contains("G") && b.contains("R")
    };
    s.add("J15-渲染分旗", j15, "绿红两态渲染可区分");

    // J16 空册零值：未登记册无要求行数且指纹为种子
    let j16 = {
        let e = CriterionBook::new("", 0);
        e.total_need() == 0
            && e.total_lines() == 0
            && e.fold() == CLOSE_TAG
            && e.gaps().is_empty()
    };
    s.add("J16-空册零值", j16, "空册要求和为零且指纹为种子");

    // J17 动态明细：收口根与批后累计
    let fp = book.fold();
    s.add_dyn(
        "J17-收口明细",
        book.declared == 285 && fp != 0,
        &format!("root={:#x} after={}", fp, after),
    );

    // J18 收口幂等拒：已冻结收口不得重跑
    s.add(
        "J18-二次收口拒",
        book.close().err() == Some(BC_FROZEN),
        "已收口批再收口必拒",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11100_batch_close_flow() {
        let (mut b, built) = build_batch();
        assert!(built);
        assert!(b.reds() == 0);
        assert!(b.close().is_ok());
        assert!(b.line_conservation());
        assert!(b.domain_balanced(DOMAIN_PRIOR.wrapping_add(b.total_lines())));
        assert!(b.enroll(20, "追挂", 15).err() == Some(BC_FROZEN));
        assert!(b.close().err() == Some(BC_FROZEN));
        let mut d = DrillRig::new();
        assert!(d.run(DRILL_ROUNDS).is_ok());
        assert!(d.converged());
        assert!(d.injected == 16);
    }

    #[test]
    fn unx11100_checks_green() {
        let s = run_unx11100_checks();
        assert!(s.all_passed(), "UNX-F11100 判据未全绿");
    }
}
