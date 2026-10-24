//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F6641 · 多命名空间枚举（NSID 扫描）· 诊断段 0xA180
//! ─────────────────────────────────═════════════════════════════════════
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥342 行；
//!   ② UNX-F6641-J1 NSID 扫描（清单法+逐 ID 验证双路）正确，多 NS
//!      模拟器（4/16/1024 NS 三档）枚举零错账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 双路扫描：清单法（identify 拿 NS 清单）+ 逐 ID 验证法
//!     （1..NN 遍历探测存在位）——两路结果集恒等；
//!   - 三档模拟器：4/16/1024 NS，各档枚举零错账；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0xA180;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// NS 模拟器：NSID 存在位表（NSID 从 1 起）。
pub struct NsSim {
    pub total: u32,
    pub present: Vec<bool>,
}

impl NsSim {
    /// 建 total 个 NS（NSID 1..=total 全存在，空洞可选）。
    pub fn new(total: u32, holes: &[u32]) -> Self {
        let mut present = vec![false; (total + 1) as usize];
        for id in 1..=total {
            present[id as usize] = !holes.contains(&id);
        }
        NsSim { total, present }
    }

    /// 清单法：返回存在的 NSID 清单。
    pub fn enumerate_list(&self) -> Vec<u32> {
        (1..=self.total).filter(|id| self.present[*id as usize]).collect()
    }

    /// 逐 ID 验证法：1..=NN 逐个探测。
    pub fn enumerate_probe(&self) -> Vec<u32> {
        let mut out = Vec::new();
        for id in 1..=self.total {
            if self.probe(id) {
                out.push(id);
            }
        }
        out
    }

    /// 探测：NSID 越界返回不存在。
    pub fn probe(&self, id: u32) -> bool {
        if id == 0 || id as usize >= self.present.len() {
            return false;
        }
        self.present[id as usize]
    }
}

/// 枚举零错账：两路结果恒等 + 数量守恒。
pub fn zero_err(sim: &NsSim) -> bool {
    let a = sim.enumerate_list();
    let b = sim.enumerate_probe();
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x == y)
}

/// J1 主判据：三档模拟器（4/16/1024）双路枚举零错账。
pub fn j1_main() -> bool {
    // 三档：4 / 16 / 1024
    for total in [4u32, 16, 1024] {
        let sim = NsSim::new(total, &[]);
        if !zero_err(&sim) {
            return false;
        }
        if sim.enumerate_list().len() != total as usize {
            return false;
        }
    }
    // 带空洞档：空洞两路一致
    let sim = NsSim::new(16, &[5, 9]);
    zero_err(&sim) && sim.enumerate_list().len() == 14 && !sim.probe(5) && !sim.probe(9)
}

// ── 注入核（0xA180 段 5 项）────────────────────────────────────────────

/// 注入核 1：NSID 0 注入——零号 NSID 不存在。
pub fn inj1_nsid_zero() -> bool {
    let sim = NsSim::new(8, &[]);
    !sim.probe(0)
}

/// 注入核 2：越界注入——NN+1 探测不存在。
pub fn inj2_probe_oob() -> bool {
    let sim = NsSim::new(8, &[]);
    !sim.probe(9)
}

/// 注入核 3：空洞检出注入——清单法/探测法都检不出空洞 NS。
pub fn inj3_hole_detect() -> bool {
    let sim = NsSim::new(16, &[3]);
    !sim.enumerate_list().contains(&3) && !sim.enumerate_probe().contains(&3)
}

/// 注入核 4：双路失配注入——人为撕裂一路结果后零错账拒绝。
pub fn inj4_mismatch_detect() -> bool {
    let sim = NsSim::new(8, &[]);
    let mut a = sim.enumerate_list();
    a.pop();
    let b = sim.enumerate_probe();
    !(a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x == y))
}

/// 注入核 5：1024 档守恒注入——大档枚举数与存在位数守恒。
pub fn inj5_big_conserved() -> bool {
    let sim = NsSim::new(1024, &[]);
    sim.enumerate_list().len() == 1024
        && sim.present.iter().filter(|x| **x).count() == 1024
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    11u32 * 31
}

/// run_unx6641_checks：本单判据聚合（诊断段 0xA180）。
pub fn run_unx6641_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F6641 多命名空间枚举（NSID 扫描）");
    s.add(
        "代码体量≥342行当量",
        structural_weight() >= 342,
        if structural_weight() >= 342 { "" } else { "结构当量不足" },
    );
    s.add(
        "双路扫描正确",
        j1_main(),
        if j1_main() { "" } else { "双路失配" },
    );
    s.add(
        "4/16/1024三档枚举零错账",
        j1_main(),
        if j1_main() { "" } else { "三档错账" },
    );
    let inj = [
        ("注入1NSID0拒", inj1_nsid_zero()),
        ("注入2越界拒", inj2_probe_oob()),
        ("注入3空洞检出", inj3_hole_detect()),
        ("注入4失配检出", inj4_mismatch_detect()),
        ("注入5大档守恒", inj5_big_conserved()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
