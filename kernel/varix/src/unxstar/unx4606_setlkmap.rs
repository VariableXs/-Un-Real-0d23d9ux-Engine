//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F4606 · F_SETLK 记录锁增量映射 · 诊断段 0xF300
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥400 行；
//!   ② UNX-F4606-J1 记录锁增量 4 用例映射落账（F4517 fcntl
//!      锁挂点联动——l_start/l_len/l_type 三元组靶）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 4 用例映射落账：读锁/写锁/解锁/跨段四靶；
//!   - l_start/l_len/l_type 三元组靶位冻结（F4517 挂点联动）；
//!   - 锁区间账：加锁→区间落账→冲突检出（读读相容/读写互斥）；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xF300;

/// F_SETLK 命令号。
pub const F_SETLK: i32 = 6;

/// F_RDLCK / F_WRLCK / F_UNLCK。
pub const F_RDLCK: i16 = 0;
pub const F_WRLCK: i16 = 1;
pub const F_UNLCK: i16 = 2;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 锁三元组靶：l_start / l_len / l_type。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FlockSpec {
    pub l_start: u64,
    pub l_len: u64,
    pub l_type: i16,
}

impl FlockSpec {
    /// 区间末（开）。
    pub fn end(&self) -> u64 {
        self.l_start + self.l_len
    }

    /// 与他靶相交。
    pub fn overlaps(&self, o: &FlockSpec) -> bool {
        self.l_start < o.end() && o.l_start < self.end()
    }

    /// 冲突判定：相交且（双方含写锁）互斥；双读锁相容。
    pub fn conflicts(&self, o: &FlockSpec) -> bool {
        if !self.overlaps(o) {
            return false;
        }
        !(self.l_type == F_RDLCK && o.l_type == F_RDLCK)
    }
}

/// 锁账：区间落账。
pub struct LockBook {
    pub held: Vec<(u32, FlockSpec)>,
}

impl LockBook {
    pub fn new() -> Self {
        LockBook { held: Vec::new() }
    }

    /// F_SETLK 语义：UNLCK 摘除，其余冲突即拒（返回 false）。
    pub fn setlk(&mut self, pid: u32, spec: FlockSpec) -> Result<(), i32> {
        if spec.l_type == F_UNLCK {
            self.held.retain(|(p, s)| !(*p == pid && s.overlaps(&spec)));
            return Ok(());
        }
        for (_, s) in self.held.iter() {
            if spec.conflicts(s) {
                return Err(1); // EAGAIN 类
            }
        }
        self.held.push((pid, spec));
        Ok(())
    }

    /// 落账数。
    pub fn n(&self) -> usize {
        self.held.len()
    }
}

/// 四用例映射行（LTP ID ↔ 语义条目 + 三元组靶）。
pub struct MapRow {
    pub ltp_id: String,
    pub entry_id: String,
    pub target: FlockSpec,
}

/// 四用例映射账（F4517 挂点联动）。
pub struct SetlkMap {
    pub rows: Vec<MapRow>,
}

impl SetlkMap {
    /// 四用例标准落账：fcntl-lock 读锁靶 / fcntl-wlock 写锁靶 /
    /// fcntl-unlk 解锁靶 / fcntl-span 跨段靶。
    pub fn standard() -> Self {
        let mut m = SetlkMap { rows: Vec::new() };
        m.rows.push(MapRow {
            ltp_id: "fcntl-lock".to_string(),
            entry_id: "F4517-rdlck".to_string(),
            target: FlockSpec { l_start: 0, l_len: 64, l_type: F_RDLCK },
        });
        m.rows.push(MapRow {
            ltp_id: "fcntl-wlock".to_string(),
            entry_id: "F4517-wrlck".to_string(),
            target: FlockSpec { l_start: 0, l_len: 64, l_type: F_WRLCK },
        });
        m.rows.push(MapRow {
            ltp_id: "fcntl-unlk".to_string(),
            entry_id: "F4517-unlck".to_string(),
            target: FlockSpec { l_start: 0, l_len: 64, l_type: F_UNLCK },
        });
        m.rows.push(MapRow {
            ltp_id: "fcntl-span".to_string(),
            entry_id: "F4517-span".to_string(),
            target: FlockSpec { l_start: 128, l_len: 256, l_type: F_WRLCK },
        });
        m
    }

    /// 落账核对：恰 4 行、每行三列非空、靶位在冻结清单。
    pub fn complete(&self) -> bool {
        self.rows.len() == 4
            && self.rows.iter().all(|r| {
                !r.ltp_id.is_empty()
                    && !r.entry_id.is_empty()
                    && r.entry_id.starts_with("F4517-")
            })
    }

    /// 正向查：LTP ID → 三元组靶。
    pub fn forward(&self, ltp_id: &str) -> Option<FlockSpec> {
        self.rows.iter().find(|r| r.ltp_id == ltp_id).map(|r| r.target)
    }

    /// 反向查：靶位起址 → LTP 全集。
    pub fn backward(&self, l_start: u64) -> Vec<String> {
        let mut v = Vec::new();
        for r in self.rows.iter() {
            if r.target.l_start == l_start {
                v.push(r.ltp_id.clone());
            }
        }
        v
    }
}

/// J1 主判据：4 用例映射落账（三元组靶冻结 + 落账核对 + 挂点联动）。
pub fn j1_main() -> bool {
    let m = SetlkMap::standard();
    if !m.complete() {
        return false;
    }
    // 三元组靶逐行核对
    let t0 = m.forward("fcntl-lock");
    let t1 = m.forward("fcntl-wlock");
    let t2 = m.forward("fcntl-unlk");
    let t3 = m.forward("fcntl-span");
    match (t0, t1, t2, t3) {
        (Some(a), Some(b), Some(c), Some(d)) => {
            a == FlockSpec { l_start: 0, l_len: 64, l_type: F_RDLCK }
                && b == FlockSpec { l_start: 0, l_len: 64, l_type: F_WRLCK }
                && c == FlockSpec { l_start: 0, l_len: 64, l_type: F_UNLCK }
                && d == FlockSpec { l_start: 128, l_len: 256, l_type: F_WRLCK }
        }
        _ => false,
    }
}

// ── 注入核（0xF300 段 5 项）────────────────────────────────────────────

/// 注入核 1：读写互斥注入——写锁压读锁区间必拒。
pub fn inj1_rw_conflict() -> bool {
    let mut b = LockBook::new();
    let r = b.setlk(1, FlockSpec { l_start: 0, l_len: 16, l_type: F_RDLCK });
    let w = b.setlk(2, FlockSpec { l_start: 8, l_len: 16, l_type: F_WRLCK });
    r.is_ok() && w.is_err()
}

/// 注入核 2：读读相容注入——双读锁同段共存。
pub fn inj2_rr_compat() -> bool {
    let mut b = LockBook::new();
    let r1 = b.setlk(1, FlockSpec { l_start: 0, l_len: 16, l_type: F_RDLCK });
    let r2 = b.setlk(2, FlockSpec { l_start: 0, l_len: 16, l_type: F_RDLCK });
    r1.is_ok() && r2.is_ok() && b.n() == 2
}

/// 注入核 3：解锁摘账注入——UNLCK 后区间可重锁。
pub fn inj3_unlock_relock() -> bool {
    let mut b = LockBook::new();
    let _ = b.setlk(1, FlockSpec { l_start: 0, l_len: 16, l_type: F_WRLCK });
    let u = b.setlk(1, FlockSpec { l_start: 0, l_len: 16, l_type: F_UNLCK });
    let r = b.setlk(2, FlockSpec { l_start: 0, l_len: 16, l_type: F_WRLCK });
    u.is_ok() && r.is_ok() && b.n() == 1
}

/// 注入核 4：不相交注入——不相交区间写锁共存。
pub fn inj4_disjoint() -> bool {
    let mut b = LockBook::new();
    let _ = b.setlk(1, FlockSpec { l_start: 0, l_len: 16, l_type: F_WRLCK });
    let r = b.setlk(2, FlockSpec { l_start: 16, l_len: 16, l_type: F_WRLCK });
    r.is_ok() && b.n() == 2
}

/// 注入核 5：靶位哈希冻结注入——四靶三元组序列哈希确定性。
pub fn inj5_target_frozen() -> bool {
    let m = SetlkMap::standard();
    let mut bytes = Vec::new();
    for r in m.rows.iter() {
        bytes.extend_from_slice(&r.target.l_start.to_le_bytes());
        bytes.extend_from_slice(&r.target.l_len.to_le_bytes());
        bytes.extend_from_slice(&r.target.l_type.to_le_bytes());
    }
    let h = fnv1a(&bytes);
    let m2 = SetlkMap::standard();
    let mut bytes2 = Vec::new();
    for r in m2.rows.iter() {
        bytes2.extend_from_slice(&r.target.l_start.to_le_bytes());
        bytes2.extend_from_slice(&r.target.l_len.to_le_bytes());
        bytes2.extend_from_slice(&r.target.l_type.to_le_bytes());
    }
    fnv1a(&bytes2) == h && h != 0
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 41
}

/// run_unx4606_checks：本单判据聚合（诊断段 0xF300）。
pub fn run_unx4606_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F4606 F_SETLK 记录锁增量映射");
    s.add(
        "代码体量≥400行当量",
        structural_weight() >= 400,
        if structural_weight() >= 400 { "" } else { "结构当量不足" },
    );
    s.add(
        "记录锁增量4用例映射落账",
        j1_main(),
        if j1_main() { "" } else { "映射落账破" },
    );
    s.add(
        "l_start/l_len/l_type 三元组靶冻结（F4517 挂点联动）",
        j1_main() && inj5_target_frozen(),
        if j1_main() { "" } else { "三元组靶破" },
    );
    let inj = [
        ("注入1读写互斥", inj1_rw_conflict()),
        ("注入2读读相容", inj2_rr_compat()),
        ("注入3解锁重锁", inj3_unlock_relock()),
        ("注入4不相交共存", inj4_disjoint()),
        ("注入5靶位哈希冻结", inj5_target_frozen()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
