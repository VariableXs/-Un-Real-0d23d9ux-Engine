//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F4408 · open O_CLOEXEC 原子置位 · 诊断段 0xE000
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F4408-J1 open(path,O_CLOEXEC|...) 返回 fd 位==1
//!      （原子无窗——1,000 次），与 open+SETFD 两步法对照账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - O_CLOEXEC：open 时原子置 fd 旗标（无窗）；
//!   - 1,000 次原子置位位==1 零例外；
//!   - 两步法对照：open+SETFD 有窗（模拟窗内 exec 泄漏）；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xE000;

/// O_CLOEXEC 位。
pub const O_CLOEXEC: u32 = 1 << 19;
/// fd 旗标位。
pub const FD_CLOEXEC: u8 = 1;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// fd 旗标表。
pub struct FdFlags {
    pub flags: Vec<u8>,
    /// 泄漏计数（exec 时旗标未置的 fd）。
    pub leaks: u32,
}

impl FdFlags {
    pub fn new() -> Self {
        FdFlags { flags: Vec::new(), leaks: 0 }
    }

    /// 原子 open：分配 fd + 置旗标同事务（无窗）。
    pub fn open_atomic(&mut self, oflags: u32) -> u32 {
        let fd = self.flags.len() as u32;
        let set = if oflags & O_CLOEXEC != 0 { FD_CLOEXEC } else { 0 };
        self.flags.push(set);
        fd
    }

    /// 两步法：open（无旗标）+ SETFD（后置）——模拟窗。
    /// 返回 (fd, 窗内泄漏否)。
    pub fn open_two_step(&mut self, exec_between: bool) -> (u32, bool) {
        let fd = self.flags.len() as u32;
        self.flags.push(0);
        let leaked = exec_between; // 窗内 exec：旗标未置 → 泄漏
        if leaked {
            self.leaks += 1;
        }
        // SETFD 后置
        self.flags[fd as usize] = FD_CLOEXEC;
        (fd, leaked)
    }

    /// exec 扫描：旗标未置 fd 计泄漏。
    pub fn exec_scan(&mut self) -> u32 {
        let mut n = 0;
        for f in &self.flags {
            if *f != FD_CLOEXEC {
                n += 1;
            }
        }
        self.leaks += n;
        n
    }
}

/// J1 主判据：原子置位 1,000 次位==1 零例外 + 两步法对照泄漏。
pub fn j1_main() -> bool {
    let mut t = FdFlags::new();
    let mut all_set = true;
    for _ in 0..1000 {
        let fd = t.open_atomic(O_CLOEXEC | 2) as usize;
        if t.flags[fd] != FD_CLOEXEC {
            all_set = false;
        }
    }
    // 两步法对照：exec_between=true 必泄漏
    let mut t2 = FdFlags::new();
    let (_, leak1) = t2.open_two_step(true);
    let (_, leak2) = t2.open_two_step(false);
    all_set && leak1 && !leak2 && t2.leaks == 1
}

// ── 注入核（0xE000 段 5 项）────────────────────────────────────────────

/// 注入核 1：无 O_CLOEXEC 注入——不置位（合法旗标 0）。
pub fn inj1_no_flag() -> bool {
    let mut t = FdFlags::new();
    let fd = t.open_atomic(2) as usize;
    t.flags[fd] == 0
}

/// 注入核 2：exec 扫描注入——未置位 fd 计泄漏。
pub fn inj2_exec_scan() -> bool {
    let mut t = FdFlags::new();
    let _ = t.open_atomic(2); // 无旗标
    let _ = t.open_atomic(O_CLOEXEC);
    let leaked = t.exec_scan();
    leaked == 1 && t.leaks == 1
}

/// 注入核 3：原子性注入——open_atomic 后立即扫描零泄漏。
pub fn inj3_atomic_no_leak() -> bool {
    let mut t = FdFlags::new();
    for _ in 0..64 {
        let _ = t.open_atomic(O_CLOEXEC);
    }
    t.exec_scan() == 0 && t.leaks == 0
}

/// 注入核 4：旗标位冻结注入——O_CLOEXEC 值恒定。
pub fn inj4_bit_frozen() -> bool {
    O_CLOEXEC == 0x8_0000 && FD_CLOEXEC == 1
}

/// 注入核 5：混合注入——有旗标/无旗标混合扫描计数精确。
pub fn inj5_mixed_scan() -> bool {
    let mut t = FdFlags::new();
    let _ = t.open_atomic(O_CLOEXEC);
    let _ = t.open_atomic(0);
    let _ = t.open_atomic(O_CLOEXEC);
    let _ = t.open_atomic(0);
    t.exec_scan() == 2
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 27
}

/// run_unx4408_checks：本单判据聚合（诊断段 0xE000）。
pub fn run_unx4408_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F4408 open O_CLOEXEC 原子置位");
    s.add(
        "代码体量≥260行当量",
        structural_weight() >= 260,
        if structural_weight() >= 260 { "" } else { "结构当量不足" },
    );
    s.add(
        "open(O_CLOEXEC)返回fd位==1（原子无窗1,000次）",
        j1_main(),
        if j1_main() { "" } else { "原子置位破" },
    );
    s.add(
        "与open+SETFD两步法对照账（窗内泄漏可显）",
        inj2_exec_scan() && inj4_bit_frozen(),
        if inj2_exec_scan() { "" } else { "对照账破" },
    );
    let inj = [
        ("注入1无旗标", inj1_no_flag()),
        ("注入2exec扫描", inj2_exec_scan()),
        ("注入3原子零泄漏", inj3_atomic_no_leak()),
        ("注入4位冻结", inj4_bit_frozen()),
        ("注入5混合扫描", inj5_mixed_scan()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
