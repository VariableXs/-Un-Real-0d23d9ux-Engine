//! UNX 卡 10149 — ENOMEM 路径正反双判据（上限达成与超限分配误用受控）
//! 段位 seg=0x105c0
//! 判据原文：UNX-F10149-J1 上限达成（限额内分配全成功 10/10 次）
//! 锚点：配额内分配全成 + 超限分配受控 ENOMEM + 释放归还配额
//! 域：QuotaMem 配额内存册（配额账 + 页债 + 归还核）
//! 结构：配额账 → 分配扣账 → 释放还账 → 超限受控 → 审计与注入检出

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x105c0;
pub const QM_ENOMEM: u32 = SEG_BASE + 0x01;
pub const QM_BAD_NPAGES: u32 = SEG_BASE + 0x02;
pub const QM_NOT_OWNED: u32 = SEG_BASE + 0x03;
pub const QM_SEALED: u32 = SEG_BASE + 0x04;
pub const QM_OVER_FREE: u32 = SEG_BASE + 0x05;

/// 配额上限（页）与债册容量
pub const QUOTA: usize = 0x40;
pub const LEDGER_CAP: usize = 0x80;

/// 一笔页债
#[derive(Clone, Copy, Debug)]
pub struct PageDebt {
    pub id: usize,
    pub pid: u32,
    pub npages: usize,
    pub live: bool,
}

/// 配额内存册
pub struct QuotaMem {
    used: usize,
    ledger: Vec<PageDebt>,
    next_id: usize,
    sealed: bool,
}

impl QuotaMem {
    pub fn new() -> Self {
        QuotaMem {
            used: 0,
            ledger: Vec::new(),
            next_id: 1,
            sealed: false,
        }
    }

    pub fn used(&self) -> usize {
        self.used
    }

    pub fn avail(&self) -> usize {
        QUOTA - self.used
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    pub fn ledger(&self) -> &[PageDebt] {
        &self.ledger
    }

    /// 分配：扣配额，超限 ENOMEM；零页受控
    pub fn alloc(&mut self, pid: u32, npages: usize) -> Result<usize, u32> {
        if self.sealed {
            return Err(QM_SEALED);
        }
        if npages == 0 {
            return Err(QM_BAD_NPAGES);
        }
        if npages > self.avail() {
            return Err(QM_ENOMEM);
        }
        if self.ledger.len() >= LEDGER_CAP {
            return Err(QM_ENOMEM);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.used += npages;
        self.ledger.push(PageDebt {
            id,
            pid,
            npages,
            live: true,
        });
        Ok(id)
    }

    /// 释放：还配额；非持有/重复释放受控
    pub fn free(&mut self, id: usize) -> Result<usize, u32> {
        if self.sealed {
            return Err(QM_SEALED);
        }
        let idx = match self.ledger.iter().position(|d| d.id == id) {
            Some(i) => i,
            None => return Err(QM_NOT_OWNED),
        };
        if !self.ledger[idx].live {
            return Err(QM_NOT_OWNED);
        }
        let n = self.ledger[idx].npages;
        if self.used < n {
            return Err(QM_OVER_FREE);
        }
        self.ledger[idx].live = false;
        self.used -= n;
        Ok(n)
    }

    /// 活债页数合计
    pub fn live_pages(&self) -> usize {
        self.ledger
            .iter()
            .filter(|d| d.live)
            .map(|d| d.npages)
            .sum()
    }

    /// 活债笔数
    pub fn live_count(&self) -> usize {
        self.ledger.iter().filter(|d| d.live).count()
    }

    /// 守恒：used == live_pages 且 used <= QUOTA
    pub fn conservation(&self) -> bool {
        self.used == self.live_pages() && self.used <= QUOTA
    }

    /// 按 pid 的活债页数
    pub fn pid_pages(&self, pid: u32) -> usize {
        self.ledger
            .iter()
            .filter(|d| d.live && d.pid == pid)
            .map(|d| d.npages)
            .sum()
    }

    /// 全还：释放某 pid 全部活债，返回归还页数
    pub fn reclaim_pid(&mut self, pid: u32) -> usize {
        let mut n = 0;
        let ids: Vec<usize> = self
            .ledger
            .iter()
            .filter(|d| d.live && d.pid == pid)
            .map(|d| d.id)
            .collect();
        for id in ids {
            if let Ok(k) = self.free(id) {
                n += k;
            }
        }
        n
    }

    /// 千分位占用
    pub fn used_permille(&self) -> usize {
        self.used * 1000 / QUOTA
    }

    /// 债册指纹
    pub fn render_ledger(&self) -> String {
        let mut s = String::new();
        for d in &self.ledger {
            let m = if d.live { "L" } else { "F" };
            s.push_str(&format!("{}:{}{};", d.id, d.npages, m));
        }
        s
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut m = QuotaMem::new();
    // 1) 超限分配必拒
    got[0] = m.alloc(1, QUOTA + 1).is_err();
    // 2) 零页分配必拒
    got[1] = m.alloc(1, 0).is_err();
    // 3) 释放非持有 id 必拒
    got[2] = m.free(999).is_err();
    // 4) 重复释放必拒
    let id = m.alloc(1, 2).unwrap_or(0);
    let _ = m.free(id);
    got[3] = m.free(id).is_err();
    // 5) 释放越配额必拒（构造 used < n 不可能于本册，改为封账后分配必拒）
    m.seal();
    got[4] = m.alloc(1, 1).is_err();
    got
}

/// 判据入口：UNX-F10149
pub fn run_unx10149_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10149");
    let mut m = QuotaMem::new();

    // J1 限额内分配全成功 10/10
    let mut j1 = true;
    for i in 0..10usize {
        if m.alloc(1, 1 + i % 3) != Ok(1 + i) {
            j1 = false;
        }
    }
    s.add("J1_alloc_10x", j1, "限额内分配 10/10 全成（id 1..10）");

    // J2 配额账推进
    let want: usize = (0..10usize).map(|i| 1 + i % 3).sum();
    s.add("J2_used", m.used() == want, "配额占用与分配和对平");

    // J3 超限分配受控
    let j3 = m.alloc(1, QUOTA + 1) == Err(QM_ENOMEM)
        && m.alloc(1, m.avail() + 1) == Err(QM_ENOMEM);
    s.add("J3_enomem", j3, "整额超限与边界超限双受控 ENOMEM");

    // J4 释放归还
    let before = m.used();
    let fr = m.free(1);
    let j4 = fr == Ok(1) && m.used() == before - 1;
    s.add("J4_free", j4, "释放还账 1 页");

    // J5 重复释放受控
    s.add("J5_double_free", m.free(1) == Err(QM_NOT_OWNED), "重复释放受控 NOT_OWNED");

    // J6 非持有 id 受控
    s.add("J6_not_owned", m.free(999) == Err(QM_NOT_OWNED), "未知 id 受控");

    // J7 零页受控
    s.add("J7_zero", m.alloc(1, 0) == Err(QM_BAD_NPAGES), "零页分配受控 BAD_NPAGES");

    // J8 守恒
    s.add("J8_conservation", m.conservation(), "used==live_pages 且不越配额");

    // J9 按 pid 归还
    let mut m2 = QuotaMem::new();
    let _ = m2.alloc(7, 5);
    let _ = m2.alloc(7, 3);
    let _ = m2.alloc(8, 2);
    let back = m2.reclaim_pid(7);
    let j9 = back == 8 && m2.used() == 2 && m2.pid_pages(7) == 0;
    s.add("J9_reclaim_pid", j9, "按 pid 全还 8 页余 2");

    // J10 注入 5 项全抓
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J10_inject_all", caught == 5, "5 注入点全检出");

    // J11 封账受控
    let mut m3 = QuotaMem::new();
    m3.seal();
    let j11 = m3.alloc(1, 1) == Err(QM_SEALED) && m3.free(1) == Err(QM_SEALED);
    s.add("J11_sealed", j11, "封账后分配/释放皆拒 SEALED");

    // J12 配额吃满再释放再分配
    let mut m4 = QuotaMem::new();
    let mut fill_ok = true;
    for _ in 0..QUOTA {
        if m4.alloc(1, 1).is_err() {
            fill_ok = false;
        }
    }
    let full = m4.alloc(1, 1) == Err(QM_ENOMEM);
    let one = m4.free(1);
    let refill = m4.alloc(1, 1);
    let j12 = fill_ok && full && one == Ok(1) && refill.is_ok();
    s.add("J12_cycle", j12, "吃满→拒→还 1→再分成功");

    // J13 id 单调
    let ids: Vec<usize> = m4.ledger().iter().map(|d| d.id).collect();
    let mut mono = true;
    for w in ids.windows(2) {
        if w[1] <= w[0] {
            mono = false;
        }
    }
    s.add("J13_id_mono", mono, "债 id 单调递增");

    // J14 千分位占用
    let pm = m.used_permille();
    let want_pm = m.used() * 1000 / QUOTA;
    s.add("J14_permille", pm == want_pm && pm > 0, "千分位占用与账对平");

    // J15 债册指纹稳定
    let fp = m.render_ledger();
    let fp2 = m.render_ledger();
    s.add("J15_fingerprint", fp == fp2 && fp.contains("1:1F;"), "债册指纹稳定含已还帧");

    // J16 活债计数
    s.add("J16_live", m.live_count() == 9 && m.live_pages() == m.used(), "活债 9 笔页数与账等");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quota49_alloc() {
        let mut m = QuotaMem::new();
        assert_eq!(m.alloc(1, 4), Ok(1));
        assert_eq!(m.used(), 4);
        assert_eq!(m.avail(), QUOTA - 4);
    }

    #[test]
    fn test_quota49_enomem() {
        let mut m = QuotaMem::new();
        assert_eq!(m.alloc(1, QUOTA + 1), Err(QM_ENOMEM));
        assert_eq!(m.alloc(1, 0), Err(QM_BAD_NPAGES));
    }

    #[test]
    fn test_quota49_free() {
        let mut m = QuotaMem::new();
        let id = m.alloc(1, 3).unwrap();
        assert_eq!(m.free(id), Ok(3));
        assert_eq!(m.free(id), Err(QM_NOT_OWNED));
        assert!(m.conservation());
    }

    #[test]
    fn test_quota49_checks() {
        let cs = run_unx10149_checks();
        assert!(cs.all_passed());
    }

    #[test]
    fn test_quota49_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
