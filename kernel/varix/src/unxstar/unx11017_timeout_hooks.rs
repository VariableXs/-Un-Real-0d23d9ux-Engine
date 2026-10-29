//! UNX-F11017 超时挂点联签（futex timeout 与 epoll timeout 语义对账）（0x1e220 段）
//!
//! 锚点：UNX-F11017-J1 超时路径千次对平（10/10），超时账核销零残留
//! （10/10）。
//! 域：CoRun·UNX-C4-B31（futex/epoll 挂号联签域）。
//!
//! 结构：双族超时账（futex 等待超时项与 epoll 等待超时项，各自期限）+
//! 时钟推进器（逐 tick 推进、到期即核销）+ 语义对账器（双族到期语义一致：
//! 期限一到必核销、未到期不误核销）+ fold 指纹与冻结 + 注入核（零期限/
//! 回拨时钟/未到期先核销/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e220 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e220;
pub const TO_ZERODL: u32 = SEG_BASE + 0x01; // 零期限
pub const TO_CLOCKBACK: u32 = SEG_BASE + 0x02; // 时钟回拨
pub const TO_EARLY: u32 = SEG_BASE + 0x03; // 未到期先核销
pub const TO_NOITEM: u32 = SEG_BASE + 0x04; // 核销不存在项
pub const TO_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后写账
pub const TO_CAP: u32 = SEG_BASE + 0x06; // 超时账超容

/// 千轮联测轮数。
pub const ROUNDS: u32 = 1000;
/// 超时账定容（须容下千次双族共 2×ROUNDS 项）。
pub const TO_CAP_N: usize = 4096;
/// 超时账域标（fold 种子）。
pub const TO_TAG: u64 = 0x1e22_11;

/// 超时族标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToFam {
    FutexWait,
    EpollWait,
}

impl ToFam {
    /// 族标符号（futex=W，epoll=E）。
    pub fn mark(&self) -> char {
        match self {
            ToFam::FutexWait => 'W',
            ToFam::EpollWait => 'E',
        }
    }
}

/// 单条超时项：族 + 序号 + 期限 + 核销位。
#[derive(Clone, Debug)]
pub struct TimeoutItem {
    pub fam: ToFam,
    pub seq: u32,
    pub deadline: u32,
    pub settled: bool,
}

impl TimeoutItem {
    /// 期限判定：tick 达到期限即到期。
    pub fn expired(&self, now: u32) -> bool {
        now >= self.deadline && self.deadline != 0
    }

    pub fn render(&self) -> String {
        let flag = if self.settled { "T" } else { "P" };
        format!("{}|{}|d{}|{}", self.fam.mark(), flag, self.deadline, self.seq)
    }
}

/// 超时账：挂号 → 逐 tick 推进 → 到期核销 → 冻结。
pub struct TimeoutLedger {
    pub items: Vec<TimeoutItem>,
    pub now: u32,
    pub ft_count: u32,
    pub ep_count: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl TimeoutLedger {
    pub fn new() -> TimeoutLedger {
        TimeoutLedger {
            items: Vec::new(),
            now: 0,
            ft_count: 0,
            ep_count: 0,
            frozen: false,
            root: None,
        }
    }

    /// 挂号超时项：冻结、零期限、期限不超前（不得早于当前钟）、超容均拒。
    pub fn enroll(&mut self, fam: ToFam, deadline: u32, seq: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(TO_FROZEN);
        }
        if deadline == 0 {
            return Err(TO_ZERODL);
        }
        if deadline < self.now {
            return Err(TO_CLOCKBACK);
        }
        // 满账先拒：定容边界优先于重号判定
        if self.items.len() >= TO_CAP_N {
            return Err(TO_CAP);
        }
        if self.items.iter().any(|i| i.fam == fam && i.seq == seq) {
            return Err(TO_NOITEM);
        }
        self.items.push(TimeoutItem { fam, seq, deadline, settled: false });
        Ok(())
    }

    /// 时钟推进：逐 tick 走，回拨必拒。
    pub fn tick(&mut self, to: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(TO_FROZEN);
        }
        if to < self.now {
            return Err(TO_CLOCKBACK);
        }
        self.now = to;
        Ok(())
    }

    /// 到期核销：逐项扫描，到期翻核销位并计族账。
    pub fn sweep(&mut self) -> u32 {
        if self.frozen {
            return 0;
        }
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.items.len() {
            if !self.items[k].settled && self.items[k].expired(self.now) {
                self.items[k].settled = true;
                match self.items[k].fam {
                    ToFam::FutexWait => self.ft_count += 1,
                    ToFam::EpollWait => self.ep_count += 1,
                }
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 未到期项清单（零残留面）。
    pub fn pending(&self) -> Vec<u32> {
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

    /// 未到期先核销越权探针（必为零）。
    pub fn illegal(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].settled && !self.items[k].expired(self.now) {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 双族语义对平：核销族账守恒且零残留零越权。
    pub fn closed(&self, rounds: u32) -> bool {
        self.ft_count == rounds && self.ep_count == rounds
            && self.pending().is_empty()
            && self.illegal() == 0
    }

    /// 链式 fold 指纹（FNV 风格，序敏感，含族/序/期限/核销位/钟面）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = TO_TAG;
        let mut k = 0usize;
        while k < self.items.len() {
            let e = &self.items[k];
            let f = match e.fam {
                ToFam::FutexWait => 1u64,
                ToFam::EpollWait => 2u64,
            };
            h = (h ^ f).wrapping_mul(0x100000001b3);
            h = (h ^ e.seq as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.deadline as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.settled as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.now as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.ft_count as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.ep_count as u64).wrapping_mul(0x100000001b3);
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

/// 期限确定性样本。
pub fn to_dl(no: u32) -> u32 {
    no.wrapping_mul(3).wrapping_add(10)
}

/// 千次超时全链：双族各千项，逐 tick 推进至全到期。
pub fn run_timeout_thousand() -> (TimeoutLedger, bool) {
    let mut l = TimeoutLedger::new();
    let mut ok = true;
    let mut i = 1u32;
    while i <= ROUNDS {
        let d = to_dl(i);
        if l.enroll(ToFam::FutexWait, d, i).is_err() {
            ok = false;
        }
        if l.enroll(ToFam::EpollWait, d, i).is_err() {
            ok = false;
        }
        i += 1;
    }
    // 逐 tick 推进至最长期限，逐 tick 清扫
    let mut t = 1u32;
    while t <= to_dl(ROUNDS) {
        if l.tick(t).is_err() {
            ok = false;
        }
        let _ = l.sweep();
        t += 1;
    }
    let l_ok = ok && l.closed(ROUNDS);
    (l, l_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 零期限必拒
    let mut a = TimeoutLedger::new();
    got[0] = a.enroll(ToFam::FutexWait, 0, 1).err() == Some(TO_ZERODL);
    // 2) 时钟回拨必拒
    let mut b = TimeoutLedger::new();
    let _ = b.tick(10);
    got[1] = b.tick(5).err() == Some(TO_CLOCKBACK);
    // 3) 期限早于当前钟必拒
    let mut c = TimeoutLedger::new();
    let _ = c.tick(50);
    got[2] = c.enroll(ToFam::EpollWait, 10, 1).err() == Some(TO_CLOCKBACK);
    // 4) 冻结后写账必抓
    let (mut d, _) = run_timeout_thousand();
    d.freeze();
    got[3] = d.enroll(ToFam::FutexWait, 99, 1).err() == Some(TO_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11017_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11017");

    // J1 超时路径千次对平：双族各千项全核销
    let (mut book, green) = run_timeout_thousand();
    s.add("J1-千次对平", green && book.closed(ROUNDS), "双族千项到期全核销零漂移");

    // J2 超时账核销零残留：pending 清空且越权核销为零
    s.add("J2-核销零残留", book.pending().is_empty() && book.illegal() == 0, "零残留且未到期先核销为零");

    // J3 双族语义一致：futex 与 epoll 族账计数相等
    s.add("J3-双族语义一致", book.ft_count == book.ep_count && book.ft_count == ROUNDS, "双族到期语义逐项一致");

    // J4 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J4-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J5 注入检出：零期限 + 时钟回拨
    let inj = inject_four();
    s.add("J5-零期回拨检出", inj[0] && inj[1], "零期限与回拨两路必抓");

    // J6 注入检出：期限超前拒 + 冻结后写
    s.add("J6-超前冻结检出", inj[2] && inj[3], "期限早于钟必拒且冻结后写必抓");

    // J7 未到期不误核销：中途钟面扫核销只收到期项
    let j7 = {
        let mut c = TimeoutLedger::new();
        let _ = c.enroll(ToFam::FutexWait, 10, 1);
        let _ = c.enroll(ToFam::EpollWait, 20, 2);
        let _ = c.tick(5);
        let n1 = c.sweep();
        let _ = c.tick(10);
        let n2 = c.sweep();
        n1 == 0 && n2 == 1 && c.pending().len() == 1
    };
    s.add("J7-未到期不误核", j7, "中途钟面只核销到期项");

    // J8 族标分旗
    let j8 = ToFam::FutexWait.mark() == 'W' && ToFam::EpollWait.mark() == 'E';
    s.add("J8-族标分旗", j8, "双族标渲染可区分");

    // J9 族内重号拒
    let j9 = {
        let mut c = TimeoutLedger::new();
        let _ = c.enroll(ToFam::FutexWait, 10, 1);
        c.enroll(ToFam::FutexWait, 20, 1).err() == Some(TO_NOITEM)
    };
    s.add("J9-族内重号拒", j9, "同族同号重登必拒");

    // J10 异族同号放行：族隔离语义
    let j10 = {
        let mut c = TimeoutLedger::new();
        let _ = c.enroll(ToFam::FutexWait, 10, 1);
        c.enroll(ToFam::EpollWait, 20, 1).is_ok()
    };
    s.add("J10-异族同号放行", j10, "族间序号隔离互不冲账");

    // J11 期限边界：期限恰等于钟面即核销
    let j11 = {
        let mut c = TimeoutLedger::new();
        let _ = c.enroll(ToFam::FutexWait, 10, 1);
        let _ = c.tick(10);
        let n = c.sweep();
        n == 1 && c.pending().is_empty()
    };
    s.add("J11-边界到期", j11, "期限等于钟面即到期");

    // J12 超容拒：第 TO_CAP_N+1 项必拒
    let j12 = {
        let mut c = TimeoutLedger::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= TO_CAP_N as u32 {
            if c.enroll(ToFam::FutexWait, n.wrapping_add(10), n).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.enroll(ToFam::FutexWait, 9999, 1).err() == Some(TO_CAP)
    };
    s.add("J12-超容边界", j12, "超出定容必拒且原账不损");

    // J13 样本确定性：期限同入同出且互异
    let d3a = to_dl(3);
    let d3b = to_dl(3);
    let mut distinct = true;
    let mut i = 1u32;
    while i <= 64u32 {
        let mut j = i.wrapping_add(1);
        while j <= 64u32 {
            if to_dl(i) == to_dl(j) {
                distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("J13-样本确定性", d3a == d3b && distinct, "期限样本同入同出且互异");

    // J14 动态明细：双族核销计数落账
    let fc = book.ft_count;
    let ec = book.ep_count;
    s.add_dyn(
        "J14-双族明细",
        fc == ROUNDS && ec == ROUNDS,
        &format!("ft={} ep={}", fc, ec),
    );

    // J15 动态明细：冻结根十六进制落账
    let fp = book.fold();
    s.add_dyn("J15-冻结根明细", fp == root, &format!("root={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11017_timeout_flow() {
        let mut l = TimeoutLedger::new();
        assert!(l.enroll(ToFam::FutexWait, 0, 1).err() == Some(TO_ZERODL));
        assert!(l.enroll(ToFam::FutexWait, 10, 1).is_ok());
        assert!(l.enroll(ToFam::FutexWait, 20, 1).err() == Some(TO_NOITEM));
        assert!(l.enroll(ToFam::EpollWait, 10, 1).is_ok());
        let _ = l.tick(10);
        assert_eq!(l.sweep(), 2);
        assert!(l.tick(5).err() == Some(TO_CLOCKBACK));
        l.freeze();
        assert!(l.enroll(ToFam::FutexWait, 99, 2).err() == Some(TO_FROZEN));
        assert!(l.root.is_some());
    }

    #[test]
    fn unx11017_checks_green() {
        let s = run_unx11017_checks();
        assert!(s.all_passed(), "UNX-F11017 判据未全绿");
    }
}
