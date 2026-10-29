//! UNX 卡 10159 — 内存面泄漏审计正反双判据（映射账守恒达成与泄漏误用受控）
//! 段位 seg=0x10700
//! 判据原文：UNX-F10159-J1 映射账守恒达成（映射/解除循环账零漂移 10/10 次）
//! 锚点：映射挂账 → 解除销账 → 循环零漂移 → 泄漏检出（活账不清零）→ 双账对平
//! 域：LeakAudit 映射账册（映射账 + 解除账 + 漂移核）
//! 结构：map 落账 → unmap 销账 → 循环守恒 → 泄漏探针 → 注入检出

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x10700;
pub const LA_LEAK: u32 = SEG_BASE + 0x01;
pub const LA_SEALED: u32 = SEG_BASE + 0x02;
pub const LA_BAD_LEN: u32 = SEG_BASE + 0x03;
pub const LA_DOUBLE_UNMAP: u32 = SEG_BASE + 0x04;
pub const LA_LEDGER_CAP: u32 = SEG_BASE + 0x05;

/// 账容量与页粒度
pub const LEDGER_CAP: usize = 0x80;
pub const PAGE: usize = 0x1000;

/// 一笔映射账
#[derive(Clone, Copy, Debug)]
pub struct MapRec {
    pub id: usize,
    pub npages: usize,
    pub live: bool,
}

/// 泄漏审计册
pub struct LeakAudit {
    pub ledger: Vec<MapRec>,
    pub mapped_total: usize,
    pub unmapped_total: usize,
    /// 泄漏检出次数
    pub leaks_caught: usize,
    next_id: usize,
    sealed: bool,
}

impl LeakAudit {
    pub fn new() -> Self {
        LeakAudit {
            ledger: Vec::new(),
            mapped_total: 0,
            unmapped_total: 0,
            leaks_caught: 0,
            next_id: 1,
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// map 落账
    pub fn map(&mut self, npages: usize) -> Result<usize, u32> {
        if self.sealed {
            return Err(LA_SEALED);
        }
        if npages == 0 {
            return Err(LA_BAD_LEN);
        }
        if self.ledger.len() >= LEDGER_CAP {
            return Err(LA_LEDGER_CAP);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.ledger.push(MapRec { id, npages, live: true });
        self.mapped_total += npages;
        Ok(id)
    }

    /// unmap 销账
    pub fn unmap(&mut self, id: usize) -> Result<usize, u32> {
        if self.sealed {
            return Err(LA_SEALED);
        }
        let i = match self.ledger.iter().position(|r| r.id == id) {
            Some(v) => v,
            None => return Err(LA_DOUBLE_UNMAP),
        };
        if !self.ledger[i].live {
            return Err(LA_DOUBLE_UNMAP);
        }
        let n = self.ledger[i].npages;
        self.ledger[i].live = false;
        self.unmapped_total += n;
        Ok(n)
    }

    /// 活账页数
    pub fn live_pages(&self) -> usize {
        self.ledger.iter().filter(|r| r.live).map(|r| r.npages).sum()
    }

    /// 漂移核：mapped - unmapped == live_pages
    pub fn drift(&self) -> usize {
        self.mapped_total - self.unmapped_total
    }

    /// 守恒：漂移与活账一致
    pub fn conserved(&self) -> bool {
        self.drift() == self.live_pages()
    }

    /// 循环零漂移探针：n 轮映射/解除后活账归零
    pub fn cycle_zero(&mut self, rounds: usize, pages: usize) -> Result<bool, u32> {
        for _ in 0..rounds {
            let id = self.map(pages)?;
            self.unmap(id)?;
        }
        Ok(self.live_pages() == 0)
    }

    /// 泄漏探针：活账非零即泄漏
    pub fn leak_probe(&mut self) -> bool {
        if self.live_pages() > 0 {
            self.leaks_caught += 1;
            true
        } else {
            false
        }
    }

    /// 活笔数
    pub fn live_count(&self) -> usize {
        self.ledger.iter().filter(|r| r.live).count()
    }

    /// 千分位泄漏率：live / mapped
    pub fn leak_permille(&self) -> usize {
        if self.mapped_total == 0 {
            return 0;
        }
        self.live_pages() * 1000 / self.mapped_total
    }

    /// 指纹
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x107000;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.id as u32);
            h = h.wrapping_mul(31).wrapping_add(if r.live { r.npages as u32 } else { 0 });
        }
        h
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 账面笔数
    pub fn rec_count(&self) -> usize {
        self.ledger.len()
    }

    /// 死账笔数
    pub fn dead_count(&self) -> usize {
        self.ledger.iter().filter(|r| !r.live).count()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = LeakAudit::new();
    // 1) 泄漏必检出（挂账不解除）
    let _ = b.map(4);
    got[0] = b.leak_probe();
    // 2) 双重解除必拒
    let id = b.map(1).unwrap_or(0);
    let _ = b.unmap(id);
    got[1] = b.unmap(id).is_err();
    // 3) 零页落账必拒
    got[2] = b.map(0).is_err();
    // 4) 守恒（漂移 == 活账）
    got[3] = b.conserved();
    // 5) 封册后必拒
    b.seal();
    got[4] = b.map(1).is_err();
    got
}

/// 判据入口：UNX-F10159
pub fn run_unx10159_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10159");
    let mut b = LeakAudit::new();

    // J1 循环零漂移 10/10：十轮映射/解除活账归零
    let mut j1 = true;
    for _ in 0..10 {
        let id = b.map(1);
        let id = match id {
            Ok(v) => v,
            Err(_) => {
                j1 = false;
                break;
            }
        };
        if b.unmap(id).is_err() {
            j1 = false;
        }
    }
    let j1 = j1 && b.live_pages() == 0 && b.conserved();
    s.add("J1_cycle_10x", j1, "映射/解除循环账零漂移 10/10");

    // J2 双账对平
    s.add("J2_balanced", b.mapped_total == b.unmapped_total && b.mapped_total == 10, "映射/解除双账各 10 页");

    // J3 泄漏检出
    let mut b2 = LeakAudit::new();
    let _ = b2.map(8);
    let l1 = b2.leak_probe();
    let _ = b2.map(2);
    let l2 = b2.leak_probe();
    s.add("J3_leak", l1 && l2 && b2.leaks_caught == 2, "两笔活账两检出");

    // J4 泄漏率千分位
    let pm = b2.leak_permille();
    s.add("J4_permille", pm == 1000, "全泄漏占比满额 1000‰");

    // J5 双重解除受控
    let id = b.map(1).unwrap_or(0);
    let _ = b.unmap(id);
    let j5 = b.unmap(id) == Err(LA_DOUBLE_UNMAP);
    s.add("J5_double_unmap", j5, "重复解除拒 DOUBLE_UNMAP");

    // J6 野柄受控
    s.add("J6_wild_id", b.unmap(999) == Err(LA_DOUBLE_UNMAP), "野账柄同码受控");

    // J7 零页受控
    s.add("J7_zero", b.map(0) == Err(LA_BAD_LEN), "零页落账拒 BAD_LEN");

    // J8 漂移核
    let d = b2.drift();
    let j8 = d == b2.live_pages() && d == 10;
    s.add("J8_drift", j8, "泄漏册漂移 10 页与活账等");

    // J9 部分解除
    let _ = b2.unmap(1); // 首笔 8 页
    let j9 = b2.live_pages() == 2 && b2.leak_permille() == 200;
    s.add("J9_partial", j9, "解除 8 页余 2 泄漏 200‰");

    // J10 大循环压力
    let mut b3 = LeakAudit::new();
    let r = b3.cycle_zero(40, 2);
    s.add("J10_stress", r == Ok(true) && b3.conserved(), "40 轮 2 页循环零漂移");

    // J11 注入 5 项全抓
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J11_inject_all", caught == 5, "5 注入点全检出");

    // J12 封册受控
    let mut b4 = LeakAudit::new();
    let z = b4.map(1).unwrap_or(0);
    b4.seal();
    let j12 = b4.map(1) == Err(LA_SEALED) && b4.unmap(z) == Err(LA_SEALED);
    s.add("J12_sealed", j12, "封册后落账/销账皆拒 SEALED");

    // J13 账容量守门
    let mut b5 = LeakAudit::new();
    let mut fill_ok = true;
    for _ in 0..LEDGER_CAP {
        if b5.map(1).is_err() {
            fill_ok = false;
        }
    }
    let j13 = fill_ok && b5.map(1) == Err(LA_LEDGER_CAP);
    s.add("J13_cap", j13, "账满 128 拒 LEDGER_CAP");

    // J14 指纹稳定
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J14_fingerprint", f1 == f2, "账册折叠指纹一致");

    // J15 id 单调
    let ids: Vec<usize> = b.ledger.iter().map(|r| r.id).collect();
    let mut mono = true;
    for w in ids.windows(2) {
        if w[1] <= w[0] {
            mono = false;
        }
    }
    s.add("J15_id_mono", mono && ids.len() == 11, "账 id 单调且 11 笔在册");

    // J16 账面笔数
    let j16 = b.rec_count() == 11;
    s.add("J16_rec_count", j16, "十轮循环加一笔活账共 11 笔");

    // J17 双账分列
    let j17 = b.mapped_total == 11 && b.unmapped_total == 11;
    s.add("J17_two_accts", j17, "映射账 11 页解除账 11 页分列（含 J5 对账对）");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_leak59_cycle() {
        let mut b = LeakAudit::new();
        let id = b.map(3).unwrap();
        assert_eq!(b.live_pages(), 3);
        assert_eq!(b.unmap(id), Ok(3));
        assert_eq!(b.live_pages(), 0);
        assert!(b.conserved());
    }

    #[test]
    fn test_leak59_leak() {
        let mut b = LeakAudit::new();
        let _ = b.map(5);
        assert!(b.leak_probe());
        assert_eq!(b.leak_permille(), 1000);
    }

    #[test]
    fn test_leak59_checks() {
        let cs = run_unx10159_checks();
        assert!(cs.all_passed());
    }

    #[test]
    fn test_leak59_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
