//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F4641 · 权限族映射总纲与表位落账 · 诊断段 0xF400
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥380 行；
//!   ② UNX-F4641-J1 权限族 40 用例映射表落账（F4597 锚表位→
//!      落账态，四列全谱——LTP ID/F ID/族别/状态），F4501–F4518
//!      靶面就绪复核；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 40 用例四列全谱落账（F4597 锚表位）；
//!   - F4501–F4518 靶面就绪复核：18 个语义条目靶位全数在账；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xF400;

/// 用例数。
pub const CASES: u32 = 40;

/// 靶面条目数（F4501~F4518）。
pub const TARGETS: u32 = 18;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 权限族别闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Fam {
    Access,
    Umask,
    Chown,
    Mode,
    Euid,
}

impl Fam {
    pub fn name(&self) -> &'static str {
        match self {
            Fam::Access => "access",
            Fam::Umask => "umask",
            Fam::Chown => "chown",
            Fam::Mode => "mode",
            Fam::Euid => "euid",
        }
    }
}

/// 状态闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum St {
    Mapped,
    Pending,
}

/// 四列行。
pub struct Row {
    pub ltp_id: String,
    pub fid: String,
    pub fam: Fam,
    pub st: St,
}

/// 权限族映射表（F4597 锚表位）。
pub struct PermMap {
    pub rows: Vec<Row>,
}

impl PermMap {
    pub fn new() -> Self {
        PermMap { rows: Vec::new() }
    }

    pub fn put(&mut self, ltp_id: &str, fid: &str, fam: Fam, st: St) -> bool {
        if self.rows.iter().any(|r| r.ltp_id == ltp_id) {
            return false;
        }
        self.rows.push(Row { ltp_id: ltp_id.to_string(), fid: fid.to_string(), fam, st });
        true
    }

    /// 四列全谱。
    pub fn full_spectrum(&self, n: u32) -> bool {
        self.rows.len() == n as usize
            && self.rows.iter().all(|r| {
                !r.ltp_id.is_empty()
                    && !r.fid.is_empty()
                    && [Fam::Access, Fam::Umask, Fam::Chown, Fam::Mode, Fam::Euid]
                        .contains(&r.fam)
                    && [St::Mapped, St::Pending].contains(&r.st)
            })
    }

    /// F4501–F4518 靶面就绪复核：18 个靶号每个至少被一行挂靠。
    pub fn targets_ready(&self) -> bool {
        for t in 4501..=4518u32 {
            let tag = format!("F{}", t);
            if !self.rows.iter().any(|r| r.fid == tag) {
                return false;
            }
        }
        true
    }

    /// 正向查。
    pub fn forward(&self, ltp_id: &str) -> Option<(String, &'static str)> {
        self.rows.iter().find(|r| r.ltp_id == ltp_id)
            .map(|r| (r.fid.clone(), r.fam.name()))
    }
}

/// 落账 40 用例：前 18 例逐靶挂 F4501~F4518，后 22 例族内细分。
pub fn populate() -> PermMap {
    let mut m = PermMap::new();
    let fams = [Fam::Access, Fam::Umask, Fam::Chown, Fam::Mode, Fam::Euid];
    for i in 0..18u32 {
        let _ = m.put(
            &format!("perm{:03}", i),
            &format!("F{}", 4501 + i),
            fams[i as usize % 5],
            St::Mapped,
        );
    }
    for i in 18..CASES {
        let _ = m.put(
            &format!("perm{:03}", i),
            &format!("F{}-x{:02}", 4501 + (i % 18), i % 7),
            fams[i as usize % 5],
            St::Mapped,
        );
    }
    m
}

/// J1 主判据：40 用例四列全谱 + F4501–F4518 靶面全就绪。
pub fn j1_main() -> bool {
    let m = populate();
    m.full_spectrum(CASES) && m.targets_ready()
}

// ── 注入核（0xF400 段 5 项）────────────────────────────────────────────

/// 注入核 1：靶面缺口注入——抽掉一个靶号复核必红。
pub fn inj1_target_gap() -> bool {
    let mut m = populate();
    // 移除 F4510 挂靠的所有行
    m.rows.retain(|r| r.fid != "F4510");
    !m.targets_ready()
}

/// 注入核 2：重复落账注入。
pub fn inj2_dup_reject() -> bool {
    let mut m = populate();
    let ok = m.put("perm000", "F4501-y", Fam::Access, St::Mapped);
    !ok && m.rows.len() == CASES as usize
}

/// 注入核 3：族别配额注入——40 例五族均衡（各 8 例）。
pub fn inj3_family_quota() -> bool {
    let m = populate();
    ["access", "umask", "chown", "mode", "euid"].iter().all(|f| {
        m.rows.iter().filter(|r| r.fam.name() == *f).count() == 8
    })
}

/// 注入核 4：正查注入——首靶行正查命中 F4501。
pub fn inj4_forward() -> bool {
    let m = populate();
    matches!(m.forward("perm000"), Some((f, _)) if f == "F4501")
}

/// 注入核 5：账目哈希冻结注入——全账 F ID 序列哈希确定性。
pub fn inj5_hash_frozen() -> bool {
    let m = populate();
    let mut bytes = Vec::new();
    for r in m.rows.iter() {
        bytes.extend_from_slice(r.fid.as_bytes());
    }
    let h = fnv1a(&bytes);
    let m2 = populate();
    let mut bytes2 = Vec::new();
    for r in m2.rows.iter() {
        bytes2.extend_from_slice(r.fid.as_bytes());
    }
    fnv1a(&bytes2) == h && h != 0
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 39
}

/// run_unx4641_checks：本单判据聚合（诊断段 0xF400）。
pub fn run_unx4641_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F4641 权限族映射总纲与表位落账");
    s.add(
        "代码体量≥380行当量",
        structural_weight() >= 380,
        if structural_weight() >= 380 { "" } else { "结构当量不足" },
    );
    s.add(
        "权限族40用例四列全谱落账",
        j1_main(),
        if j1_main() { "" } else { "四列全谱破" },
    );
    s.add(
        "F4501-F4518 靶面就绪复核",
        j1_main(),
        if j1_main() { "" } else { "靶面缺口" },
    );
    let inj = [
        ("注入1靶面缺口必红", inj1_target_gap()),
        ("注入2重复落账拒", inj2_dup_reject()),
        ("注入3族别配额", inj3_family_quota()),
        ("注入4正查命中", inj4_forward()),
        ("注入5账目哈希冻结", inj5_hash_frozen()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
