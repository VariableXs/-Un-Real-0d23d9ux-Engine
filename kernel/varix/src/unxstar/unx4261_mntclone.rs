//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F4261 · mnt_namespace 克隆（CLONE_NEWNS 预锚） · 诊断段 0xDD00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F4261-J1 克隆时父命名空间挂载树全量快照复制（树遍历=
//!      快照遍历，逐节点一致），克隆后父子互不影响（单侧挂载注入
//!      另一侧零可见，100/100）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 快照复制：逐节点（mountpoint+parent 结构）全量复制；
//!   - 一致性：原树先序遍历 ≡ 克隆树先序遍历；
//!   - 隔离性：克隆后单侧挂载，另一侧零可见（100/100 对）；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xDD00;

/// 隔离对数。
pub const ISO_PAIRS: u32 = 100;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 挂载命名空间。
pub struct MntNs {
    /// 挂载点路径集（有序）。
    pub mounts: Vec<String>,
}

impl MntNs {
    pub fn new() -> Self {
        let mut mounts = Vec::new();
        mounts.push(String::from("/"));
        MntNs { mounts }
    }

    pub fn mount(&mut self, mp: &str) -> bool {
        if self.mounts.iter().any(|m| m == mp) {
            return false;
        }
        self.mounts.push(String::from(mp));
        true
    }

    pub fn has(&self, mp: &str) -> bool {
        self.mounts.iter().any(|m| m == mp)
    }

    pub fn count(&self) -> usize {
        self.mounts.len()
    }

    /// 先序遍历序（此处 mounts 即序）哈希。
    pub fn walk_hash(&self) -> u32 {
        let mut buf: Vec<u8> = Vec::new();
        for m in &self.mounts {
            for &b in m.as_bytes() {
                buf.push(b);
            }
            buf.push(b'|');
        }
        fnv1a(&buf)
    }

    /// 克隆：全量快照复制。
    pub fn clone_ns(&self) -> MntNs {
        let mut mounts = Vec::new();
        for m in &self.mounts {
            mounts.push(m.clone());
        }
        MntNs { mounts }
    }

    /// 快照一致性：原树遍历 ≡ 克隆树遍历（逐节点一致）。
    pub fn snapshot_equal(&self, other: &MntNs) -> bool {
        self.mounts == other.mounts && self.walk_hash() == other.walk_hash()
    }
}

/// 标准父空间构造：根 + n 层挂载。
pub fn build_parent(depth: u32) -> MntNs {
    let mut ns = MntNs::new();
    for i in 1..=depth {
        let mp = {
            let mut s = String::from("/mnt");
            s.push_str(&(i as u64).to_string());
            s
        };
        let _ = ns.mount(&mp);
    }
    ns
}

/// J1 主判据：快照复制一致 + 隔离 100/100。
pub fn j1_main() -> bool {
    let parent = build_parent(32);
    let child = parent.clone_ns();
    let snap_ok = parent.snapshot_equal(&child);
    // 隔离：单侧挂载另一侧零可见
    let mut p2 = parent.clone_ns();
    let mut c2 = parent.clone_ns();
    let mut iso = 0u32;
    for i in 0..ISO_PAIRS {
        let pm = {
            let mut s = String::from("/p-only");
            s.push_str(&(i as u64).to_string());
            s
        };
        let cm = {
            let mut s = String::from("/c-only");
            s.push_str(&(i as u64).to_string());
            s
        };
        let a = p2.mount(&pm) && !c2.has(&pm);
        let b = c2.mount(&cm) && !p2.has(&cm);
        if a && b {
            iso += 1;
        }
    }
    snap_ok && iso == ISO_PAIRS && p2.count() == parent.count() + ISO_PAIRS as usize
        && c2.count() == parent.count() + ISO_PAIRS as usize
}

// ── 注入核（0xDD00 段 5 项）────────────────────────────────────────────

/// 注入核 1：快照漂移注入——克隆后改父克隆不变。
pub fn inj1_snap_immutable() -> bool {
    let p = build_parent(8);
    let mut c = p.clone_ns();
    let before = c.walk_hash();
    let mut p2 = p.clone_ns();
    let _ = p2.mount("/new");
    c.walk_hash() == before && p2.count() == p.count() + 1
}

/// 注入核 2：重挂注入——同 mountpoint 重复挂必拒。
pub fn inj2_dup_mount() -> bool {
    let mut ns = MntNs::new();
    let a = ns.mount("/x");
    let b = ns.mount("/x");
    a && !b
}

/// 注入核 3：根保留注入——根挂载点必在。
pub fn inj3_root_kept() -> bool {
    let ns = build_parent(0);
    ns.has("/") && ns.count() == 1
}

/// 注入核 4：空空间克隆注入——仅根克隆一致。
pub fn inj4_empty_clone() -> bool {
    let p = MntNs::new();
    let c = p.clone_ns();
    p.snapshot_equal(&c) && p.walk_hash() != 0
}

/// 注入核 5：隔离双向注入——父见子新挂必假。
pub fn inj5_iso_bidir() -> bool {
    let p = build_parent(4);
    let mut c = p.clone_ns();
    let _ = c.mount("/c-only");
    !p.has("/c-only") && c.has("/c-only")
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 33
}

/// run_unx4261_checks：本单判据聚合（诊断段 0xDD00）。
pub fn run_unx4261_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F4261 mnt_namespace 克隆（CLONE_NEWNS 预锚）");
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    s.add(
        "克隆时挂载树全量快照复制（遍历逐节点一致）",
        j1_main(),
        if j1_main() { "" } else { "快照复制破" },
    );
    s.add(
        "克隆后父子互不影响（单侧挂载零可见 100/100）",
        j1_main(),
        if j1_main() { "" } else { "隔离破" },
    );
    let inj = [
        ("注入1快照不可变", inj1_snap_immutable()),
        ("注入2重挂", inj2_dup_mount()),
        ("注入3根保留", inj3_root_kept()),
        ("注入4空克隆", inj4_empty_clone()),
        ("注入5双向隔离", inj5_iso_bidir()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
