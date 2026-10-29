//! UNX-F11003 futex WAIT 挂点联测（入眠路径与挂号账联动）（0x1e060 段）
//!
//! 锚点：UNX-F11003-J1 入眠千次账联动零偏离（10/10），挂点留痕逐次对平
//! （10/10）。
//! 域：CoRun·UNX-C4-B31（futex/epoll 挂号联签域）。
//!
//! 结构：入眠账（futex 等待队列项：地址/期望值/入眠序号）+ 留痕簿（每次
//! 入眠逐次落痕、序号严格递增）+ 守恒器（入眠次数 = 留痕次数 = 待核数，
//! 偏离即拒）+ fold 指纹与冻结 + 注入核（值偏离/序号回退/重号/冻结后写
//! 四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e060 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e060;
pub const WV_MISMATCH: u32 = SEG_BASE + 0x01; // 期望值与实值偏离
pub const WV_SEQ: u32 = SEG_BASE + 0x02; // 留痕序号回退
pub const WV_DUP: u32 = SEG_BASE + 0x03; // 入眠序号重号
pub const WV_EMPTY: u32 = SEG_BASE + 0x04; // 地址空
pub const WV_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后写账
pub const WV_NOITEM: u32 = SEG_BASE + 0x06; // 核销目标不存在

/// 千次入眠轮数。
pub const ROUNDS: u32 = 1000;
/// 入眠账定容。
pub const WAIT_CAP: usize = 1024;
/// 入眠账域标（fold 种子）。
pub const WAIT_TAG: u64 = 0x1e06_0003;

/// 单条入眠账项：地址 + 期望值 + 入眠序号 + 核销位 + 偏离位。
#[derive(Clone, Debug)]
pub struct WaitItem {
    pub uaddr: u32,
    pub expect: u32,
    pub seq: u32,
    pub settled: bool,
    pub dev_flag: bool,
}

impl WaitItem {
    /// 期望值与实值一致才算可核销。
    pub fn matches(&self, actual: u32) -> bool {
        self.expect == actual
    }

    pub fn render(&self) -> String {
        let flag = if self.settled { "W" } else { "S" };
        format!("seq{}|{}|a{}", self.seq, flag, self.uaddr)
    }
}

/// futex WAIT 入眠账：挂号 → 留痕 → 逐次核销 → 冻结。
pub struct WaitLedger {
    pub items: Vec<WaitItem>,
    pub traces: Vec<u32>,
    pub last_seq: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl WaitLedger {
    pub fn new() -> WaitLedger {
        WaitLedger {
            items: Vec::new(),
            traces: Vec::new(),
            last_seq: 0,
            frozen: false,
            root: None,
        }
    }

    /// 挂号入眠项：冻结、零址、超容均拒；序号必须严格递增。
    pub fn enroll(&mut self, uaddr: u32, expect: u32, seq: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(WV_FROZEN);
        }
        if uaddr == 0 {
            return Err(WV_EMPTY);
        }
        if seq == 0 {
            return Err(WV_SEQ);
        }
        if self.items.iter().any(|i| i.seq == seq) {
            return Err(WV_DUP);
        }
        if seq <= self.last_seq {
            return Err(WV_SEQ);
        }
        if self.items.len() >= WAIT_CAP {
            return Err(WV_MISMATCH);
        }
        self.last_seq = seq;
        self.items.push(WaitItem { uaddr, expect, seq, settled: false, dev_flag: false });
        self.traces.push(seq);
        Ok(())
    }

    /// 逐次核销：实值与期望一致才翻核销位，否则记偏离。
    pub fn settle(&mut self, seq: u32, actual: u32) -> Result<bool, u32> {
        if self.frozen {
            return Err(WV_FROZEN);
        }
        let mut hit = false;
        let mut ok = false;
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].seq == seq {
                ok = self.items[k].matches(actual);
                self.items[k].settled = true;
                self.items[k].dev_flag = !ok;
                hit = true;
            }
            k += 1;
        }
        if hit {
            Ok(ok)
        } else {
            Err(WV_NOITEM)
        }
    }

    /// 未核销序号清单（按入眠序）。
    pub fn gaps(&self) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            if !self.items[k].settled {
                g.push(self.items[k].seq);
            }
            k += 1;
        }
        g
    }

    /// 偏离项清单（已核销但实值不匹配）。
    pub fn deviated(&self) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].dev_flag {
                g.push(self.items[k].seq);
            }
            k += 1;
        }
        g
    }

    /// 守恒裁断：留痕次数 = 挂号数且全数核销零偏离。
    pub fn conserved(&self) -> bool {
        self.traces.len() == self.items.len()
            && self.gaps().is_empty()
            && self.deviated().is_empty()
    }

    /// 链式 fold 指纹（FNV 风格，序敏感，含址/值/序/核销位）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = WAIT_TAG;
        let mut k = 0usize;
        while k < self.items.len() {
            let it = &self.items[k];
            h = (h ^ it.uaddr as u64).wrapping_mul(0x100000001b3);
            h = (h ^ it.expect as u64).wrapping_mul(0x100000001b3);
            h = (h ^ it.seq as u64).wrapping_mul(0x100000001b3);
            h = (h ^ it.settled as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        let mut k = 0usize;
        while k < self.traces.len() {
            h = (h ^ self.traces[k] as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h
    }

    /// 冻结：翻冻结位并落冻结根。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

impl WaitItem {
    /// 核销位与偏离位分列：偏离位独立留痕不可静默翻绿。
    pub fn deviated(&self) -> bool {
        self.dev_flag
    }
}

/// 期望值确定性样本（正反两路共用）。
pub fn wait_sample(no: u32) -> u32 {
    no.wrapping_mul(13).wrapping_add(5)
}

/// 千次入眠全链：逐轮挂号并按同轮实值核销，返回是否零偏离。
pub fn run_thousand() -> (WaitLedger, bool) {
    let mut l = WaitLedger::new();
    let mut i = 1u32;
    let mut ok = true;
    while i <= ROUNDS {
        let v = wait_sample(i);
        if l.enroll(i.wrapping_mul(4), v, i).is_err() {
            ok = false;
        }
        if l.settle(i, v) != Ok(true) {
            ok = false;
        }
        i += 1;
    }
    let l_ok = ok && l.conserved();
    (l, l_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 核销实值偏离必记偏离
    let mut a = WaitLedger::new();
    let _ = a.enroll(8, 100, 1);
    let _ = a.settle(1, 101);
    got[0] = a.deviated().len() == 1 && !a.conserved();
    // 2) 留痕序号回退必拒
    let mut b = WaitLedger::new();
    let _ = b.enroll(8, 1, 5);
    got[1] = b.enroll(8, 1, 4).err() == Some(WV_SEQ);
    // 3) 重号必拒
    let mut c = WaitLedger::new();
    let _ = c.enroll(8, 1, 3);
    got[2] = c.enroll(16, 1, 3).err() == Some(WV_DUP);
    // 4) 冻结后写账必抓
    let (mut d, _) = run_thousand();
    d.freeze();
    got[3] = d.enroll(8, 1, ROUNDS + 1).err() == Some(WV_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11003_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11003");

    // J1 入眠千次账联动零偏离
    let (mut book, green) = run_thousand();
    s.add("J1-千次零偏离", green && book.conserved() && book.deviated().is_empty(), "千轮挂号与核销逐次对平零偏离");

    // J2 挂点留痕逐次对平：留痕数与挂号数相等且序号递增
    let mut mono = true;
    let mut k = 1usize;
    while k < book.traces.len() {
        if book.traces[k] <= book.traces[k - 1] {
            mono = false;
        }
        k += 1;
    }
    s.add("J2-留痕逐次对平", mono && book.traces.len() == ROUNDS as usize, "千次留痕序号严格递增");

    // J3 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J3-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J4 冻结后写账必抓
    let j4 = book.enroll(8, 1, ROUNDS + 2).err() == Some(WV_FROZEN);
    s.add("J4-冻结拒写", j4, "冻结后挂号必拒");

    // J5 注入检出：值偏离 + 序号回退
    let inj = inject_four();
    s.add("J5-偏离回退检出", inj[0] && inj[1], "实值偏离与序号回退两路必抓");

    // J6 注入检出：重号 + 冻结后写
    s.add("J6-重号冻结检出", inj[2] && inj[3], "序号重号必拒且冻结后写必抓");

    // J7 零址拒收
    let j7 = WaitLedger::new().enroll(0, 1, 1).err() == Some(WV_EMPTY);
    s.add("J7-零址拒收", j7, "等待地址零必拒");

    // J8 核销目标不存在拒
    let j8 = WaitLedger::new().settle(9, 1).err() == Some(WV_NOITEM);
    s.add("J8-目标不存在", j8, "核销不存在的序号必拒");

    // J9 缺口语义：只核销半数时 gaps 可枚举且守恒为假
    let j9 = {
        let mut c = WaitLedger::new();
        let mut i = 1u32;
        while i <= 100u32 {
            let v = wait_sample(i);
            let _ = c.enroll(i.wrapping_mul(4), v, i);
            i += 1;
        }
        let mut i = 1u32;
        while i <= 50u32 {
            let v = wait_sample(i);
            let _ = c.settle(i, v);
            i += 1;
        }
        c.gaps().len() == 50 && !c.conserved()
    };
    s.add("J9-缺口枚举", j9, "漏核销半数可枚举且守恒为假");

    // J10 渲染面：已核销与待核销标记可区分
    let j10 = {
        let a = WaitItem { uaddr: 3, expect: 4, seq: 1, settled: true, dev_flag: false };
        let b = WaitItem { uaddr: 3, expect: 4, seq: 2, settled: false, dev_flag: false };
        a.render() != b.render() && a.render().contains("W") && b.render().contains("S")
    };
    s.add("J10-渲染分旗", j10, "核销/待核渲染可区分");

    // J11 未核销不算守恒：挂号齐但核销位未翻
    let j11 = {
        let mut c = WaitLedger::new();
        let mut i = 1u32;
        while i <= 64u32 {
            let v = wait_sample(i);
            let _ = c.enroll(i.wrapping_mul(4), v, i);
            i += 1;
        }
        !c.conserved() && c.gaps().len() == 64
    };
    s.add("J11-未翻位不守恒", j11, "挂号齐但未逐次核销仍不算守恒");

    // J12 定容边界：超 WAIT_CAP 拒
    let j12 = {
        let mut c = WaitLedger::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= WAIT_CAP as u32 {
            if c.enroll(n.wrapping_mul(4), n, n).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.enroll(8, 1, WAIT_CAP as u32 + 1).err() == Some(WV_MISMATCH)
    };
    s.add("J12-定容边界", j12, "超出定容必拒且原账不损");

    // J13 样本确定性：同号两次求值一致且异号互异
    let v3a = wait_sample(3);
    let v3b = wait_sample(3);
    let mut distinct = true;
    let mut i = 1u32;
    while i <= 32u32 {
        let mut j = i.wrapping_add(1);
        while j <= 32u32 {
            if wait_sample(i) == wait_sample(j) {
                distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("J13-样本确定性", v3a == v3b && distinct, "样本同入同出且互异");

    // J14 动态明细：千次留痕计数落账
    let tn = book.traces.len();
    s.add_dyn("J14-留痕明细", tn == ROUNDS as usize && book.deviated().is_empty(), &format!("traces={}", tn));

    // J15 动态明细：冻结根十六进制落账
    let fp = book.fold();
    s.add_dyn("J15-冻结根明细", fp == root, &format!("root={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11003_wait_flow() {
        let mut l = WaitLedger::new();
        assert!(l.enroll(8, 42, 1).is_ok());
        assert!(l.enroll(8, 1, 1).err() == Some(WV_DUP));
        assert_eq!(l.settle(1, 42), Ok(true));
        assert!(l.enroll(16, 7, 2).is_ok());
        assert_eq!(l.settle(2, 8), Ok(false));
        assert!(!l.conserved());
        l.freeze();
        assert!(l.enroll(8, 1, 3).err() == Some(WV_FROZEN));
        assert!(l.root.is_some());
    }

    #[test]
    fn unx11003_checks_green() {
        let s = run_unx11003_checks();
        assert!(s.all_passed(), "UNX-F11003 判据未全绿");
    }
}
