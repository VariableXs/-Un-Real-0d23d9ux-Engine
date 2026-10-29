//! UNX 卡 10145 — brk 语义档正反双判据（伸缩语义达成与非法收缩误用受控）
//! 段位 seg=0x10540
//! 判据原文：UNX-F10145-J1 伸缩语义达成（推进/收缩/回探语义 LTP 面 10/10 次）
//! 锚点：heap 顶针 break 的推进/收缩/回探三语义 + 语义档轨迹账
//! 域：BrkLedger 语义档册（每次 brk 调用落一档，方向/起点/终点/返回值四元）
//! 结构：base 顶针 limit → advance/shrink 正路径 → probe 回探预演不落针 → 档账审计与注入检出

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x10540;
pub const BR45_BAD_STEP: u32 = SEG_BASE + 0x01;
pub const BR45_UNDER_BASE: u32 = SEG_BASE + 0x02;
pub const BR45_OVER_LIMIT: u32 = SEG_BASE + 0x03;
pub const BR45_SEALED: u32 = SEG_BASE + 0x04;
pub const BR45_LEDGER_FULL: u32 = SEG_BASE + 0x05;

/// 页粒度：brk 语义档的对齐基元
pub const PAGE: usize = 0x1000;
/// 语义档账容量上限（容量先定后用）
pub const LEDGER_CAP: usize = 0x40;

/// brk 调用方向：推进 / 收缩 / 回探（预演不落针）
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BrkDir {
    Grow,
    Shrink,
    Probe,
}

/// 单档语义事件：方向、起点、终点、实际返回
#[derive(Clone, Copy, Debug)]
pub struct BrkEvent {
    pub dir: BrkDir,
    pub from: usize,
    pub to: usize,
    pub ret: usize,
}

/// brk 语义档册
pub struct BrkLedger {
    base: usize,
    brk: usize,
    limit: usize,
    ledger: Vec<BrkEvent>,
    sealed: bool,
}

impl BrkLedger {
    pub fn new(base: usize, limit: usize) -> Self {
        BrkLedger {
            base,
            brk: base,
            limit,
            ledger: Vec::new(),
            sealed: false,
        }
    }

    pub fn brk(&self) -> usize {
        self.brk
    }

    pub fn base(&self) -> usize {
        self.base
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 页对齐上取整（brk 返回值恒为对齐地址）
    pub fn align_up(a: usize) -> usize {
        (a + PAGE - 1) & !(PAGE - 1)
    }

    /// 推进语义：step>0，目标 = brk+step，越限即拒
    pub fn advance(&mut self, step: usize) -> Result<usize, u32> {
        if self.sealed {
            return Err(BR45_SEALED);
        }
        if step == 0 {
            return Err(BR45_BAD_STEP);
        }
        let want = self.brk + step;
        if want > self.limit {
            return Err(BR45_OVER_LIMIT);
        }
        if self.ledger.len() >= LEDGER_CAP {
            return Err(BR45_LEDGER_FULL);
        }
        let from = self.brk;
        self.brk = want;
        self.ledger.push(BrkEvent {
            dir: BrkDir::Grow,
            from,
            to: want,
            ret: want,
        });
        Ok(want)
    }

    /// 收缩语义：目标 = brk-step，不得低于 base
    pub fn shrink(&mut self, step: usize) -> Result<usize, u32> {
        if self.sealed {
            return Err(BR45_SEALED);
        }
        if step == 0 {
            return Err(BR45_BAD_STEP);
        }
        if step > self.brk - self.base {
            return Err(BR45_UNDER_BASE);
        }
        if self.ledger.len() >= LEDGER_CAP {
            return Err(BR45_LEDGER_FULL);
        }
        let from = self.brk;
        self.brk -= step;
        self.ledger.push(BrkEvent {
            dir: BrkDir::Shrink,
            from,
            to: self.brk,
            ret: self.brk,
        });
        Ok(self.brk)
    }

    /// 回探语义：预演推进(正 delta)/收缩(负 delta)，不改顶针，但落 Probe 档
    pub fn probe(&mut self, delta: i64) -> Result<usize, u32> {
        if self.sealed {
            return Err(BR45_SEALED);
        }
        if delta == 0 {
            return Err(BR45_BAD_STEP);
        }
        if self.ledger.len() >= LEDGER_CAP {
            return Err(BR45_LEDGER_FULL);
        }
        let cur = self.brk as i64;
        let want = cur + delta;
        if want < self.base as i64 {
            return Err(BR45_UNDER_BASE);
        }
        if want > self.limit as i64 {
            return Err(BR45_OVER_LIMIT);
        }
        let want = want as usize;
        self.ledger.push(BrkEvent {
            dir: BrkDir::Probe,
            from: self.brk,
            to: want,
            ret: Self::align_up(want),
        });
        Ok(Self::align_up(want))
    }

    /// 档账计数：(推进, 收缩, 回探)
    pub fn audit(&self) -> (usize, usize, usize) {
        let mut g = 0;
        let mut s = 0;
        let mut p = 0;
        for e in &self.ledger {
            match e.dir {
                BrkDir::Grow => g += 1,
                BrkDir::Shrink => s += 1,
                BrkDir::Probe => p += 1,
            }
        }
        (g, s, p)
    }

    /// 净漂移：当前顶针相对 base 的偏移
    pub fn drift(&self) -> usize {
        self.brk - self.base
    }

    /// 对账：净漂移 = 推进总量 − 收缩总量（回探不计）
    pub fn conservation(&self) -> bool {
        let mut grown = 0usize;
        let mut shrunk = 0usize;
        for e in &self.ledger {
            match e.dir {
                BrkDir::Grow => grown += e.to - e.from,
                BrkDir::Shrink => shrunk += e.from - e.to,
                BrkDir::Probe => {}
            }
        }
        grown - shrunk == self.drift()
    }

    /// 档账末帧一致性：末档 to 与当前顶针一致（回探除外）
    pub fn tail_consistent(&self) -> bool {
        match self.ledger.last() {
            None => self.brk == self.base,
            Some(e) => e.dir == BrkDir::Probe || e.to == self.brk,
        }
    }

    /// 封账：之后一切伸缩回探皆拒
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 轨迹渲染：每档一行 G/S/P 方向字母
    pub fn render_track(&self) -> String {
        let mut s = String::new();
        for e in &self.ledger {
            let d = match e.dir {
                BrkDir::Grow => "G",
                BrkDir::Shrink => "S",
                BrkDir::Probe => "P",
            };
            s.push_str(&format!("{}:{:#x}->{:#x};", d, e.from, e.to));
        }
        s
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut led = BrkLedger::new(0x4000, 0x8000);
    // 1) 零步长推进必拒
    got[0] = led.advance(0).is_err();
    // 2) 收缩穿底必拒
    got[1] = led.shrink(0x9000).is_err();
    // 3) 回探不得动顶针
    let before = led.brk();
    let _ = led.probe(0x100);
    got[2] = led.brk() == before;
    // 4) 回探零步长必拒
    got[3] = led.probe(0).is_err();
    // 5) 封账后推进必拒
    led.seal();
    got[4] = led.advance(0x100).is_err();
    got
}

/// 判据入口：UNX-F10145
pub fn run_unx10145_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10145");
    let mut led = BrkLedger::new(0x10000, 0x20000);

    // J1 推进语义 10/10
    let mut j1 = true;
    for i in 1..=10usize {
        let want = 0x10000 + i * PAGE;
        if led.advance(PAGE) != Ok(want) {
            j1 = false;
        }
    }
    s.add("J1_grow_10x", j1, "推进语义 10/10 达成");

    // J2 收缩语义：连收 5 页
    let mut j2 = true;
    for i in (5..10).rev() {
        let want = 0x10000 + i * PAGE;
        if led.shrink(PAGE) != Ok(want) {
            j2 = false;
        }
    }
    s.add("J2_shrink_5x", j2, "收缩语义 5/5 达成");

    // J3 回探不改顶针
    let cur = led.brk();
    let pv = led.probe(PAGE as i64);
    let j3 = pv == Ok(cur + PAGE) && led.brk() == cur;
    s.add("J3_probe_no_move", j3, "回探预演返回对齐目标且顶针不动");

    // J4 档账审计对平：10 推 + 5 收 + 1 探
    let (g, sh, p) = led.audit();
    s.add("J4_audit_bal", g == 10 && sh == 5 && p == 1, "档账 10G/5S/1P 对平");

    // J5 非法收缩：一步穿底
    let big = led.brk() - led.base() + PAGE;
    let j5 = led.shrink(big) == Err(BR45_UNDER_BASE) && led.brk() == cur;
    s.add("J5_shrink_under_base", j5, "穿底收缩受控为 UNDER_BASE");

    // J6 越限推进
    let step_to_limit = led.limit() - led.brk() + PAGE;
    let j6 = led.advance(step_to_limit) == Err(BR45_OVER_LIMIT);
    s.add("J6_grow_over_limit", j6, "越限推进受控为 OVER_LIMIT");

    // J7 零步长双拒
    let j7 = led.advance(0) == Err(BR45_BAD_STEP) && led.shrink(0) == Err(BR45_BAD_STEP);
    s.add("J7_zero_step", j7, "零步长推进/收缩双拒 BAD_STEP");

    // J8 对齐语义
    s.add("J8_align_up", BrkLedger::align_up(0x1234) == 0x2000, "align_up 页上取整");

    // J9 守恒：净漂移 = 推进总量 − 收缩总量
    let j9 = led.conservation() && led.drift() == 5 * PAGE;
    s.add("J9_drift_conserved", j9, "净漂移 5 页与档账守恒");

    // J10 尾档一致性
    s.add("J10_tail", led.tail_consistent(), "末档 to 与顶针一致");

    // J11 注入 5 项全抓
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J11_inject_all", caught == 5, "5 注入点全检出");

    // J12 封账受控
    let mut led2 = BrkLedger::new(0x1000, 0x4000);
    led2.seal();
    let j12 = led2.advance(PAGE) == Err(BR45_SEALED)
        && led2.shrink(PAGE) == Err(BR45_SEALED)
        && led2.probe(PAGE as i64) == Err(BR45_SEALED);
    s.add("J12_sealed", j12, "封账后三语义皆拒 SEALED");

    // J13 回探越界预演受控
    let mut led3 = BrkLedger::new(0x1000, 0x2000);
    let j13 = led3.probe(-0x2000) == Err(BR45_UNDER_BASE)
        && led3.probe(0x2000) == Err(BR45_OVER_LIMIT);
    s.add("J13_probe_bounds", j13, "回探双向越界受控");

    // J14 档账容量守门
    let mut led4 = BrkLedger::new(0x1000, 0x100000);
    let mut fill_ok = true;
    for _ in 0..LEDGER_CAP {
        if led4.advance(PAGE).is_err() {
            fill_ok = false;
        }
    }
    let j14 = fill_ok && led4.advance(PAGE) == Err(BR45_LEDGER_FULL);
    s.add("J14_ledger_cap", j14, "档账 64 满后拒 LEDGER_FULL");

    // J15 轨迹指纹稳定
    let mut led5 = BrkLedger::new(0x1000, 0x8000);
    let _ = led5.advance(0x300);
    let _ = led5.probe(-0x100);
    let _ = led5.shrink(0x100);
    let fp = led5.render_track();
    let fp2 = led5.render_track();
    s.add("J15_fingerprint", fp == fp2 && fp.contains("G:") && fp.contains("P:") && fp.contains("S:"), "轨迹指纹 G/P/S 齐备且稳定");

    s.add("J16_brk_immutable_on_err", led.brk() == 0x10000 + 5 * PAGE, "全部误用后顶针不动");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_brk45_semantics() {
        let mut led = BrkLedger::new(0x1000, 0x4000);
        assert_eq!(led.advance(0x200), Ok(0x1200));
        assert_eq!(led.shrink(0x100), Ok(0x1100));
        assert_eq!(led.probe(0x40), Ok(0x1200));
        assert_eq!(led.brk(), 0x1100);
        assert!(led.conservation());
    }

    #[test]
    fn test_brk45_misuse() {
        let mut led = BrkLedger::new(0x1000, 0x2000);
        assert_eq!(led.advance(0x8000), Err(BR45_OVER_LIMIT));
        assert_eq!(led.shrink(0x10), Err(BR45_UNDER_BASE));
        assert_eq!(led.advance(0), Err(BR45_BAD_STEP));
    }

    #[test]
    fn test_brk45_checks() {
        let cs = run_unx10145_checks();
        assert!(cs.all_passed());
    }

    #[test]
    fn test_brk45_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
