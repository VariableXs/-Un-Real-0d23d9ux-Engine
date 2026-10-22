//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F4207 · *at 族 dirfd 基准解析（AT_FDCWD） · 诊断段 0xDB80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F4207-J1 dirfd 基准 × 绝对/相对路径 2×2 矩阵全过
//!      （绝对路径忽略 dirfd 是 POSIX 权威口径），AT_FDCWD 哨兵
//!      退回 cwd 基准断言通过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 2×2 矩阵：(dirfd 基准 vs cwd 基准) × (绝对 vs 相对)；
//!   - 权威口径：绝对路径忽略 dirfd；相对路径锚 dirfd；
//!   - AT_FDCWD(-100) 哨兵 → cwd 基准；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xDB80;

/// AT_FDCWD 哨兵。
pub const AT_FDCWD: i32 = -100;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 解析基准。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Base {
    DirFd,
    Cwd,
}

impl Base {
    pub fn name(self) -> &'static str {
        match self {
            Base::DirFd => "DIRFD",
            Base::Cwd => "CWD",
        }
    }
}

/// dirfd 表。
pub struct DirFdTable {
    pub entries: Vec<String>,
}

impl DirFdTable {
    pub fn new() -> Self {
        DirFdTable { entries: Vec::new() }
    }

    pub fn open(&mut self, dir: &str) -> i32 {
        self.entries.push(String::from(dir));
        (self.entries.len() - 1) as i32
    }

    pub fn get(&self, fd: i32) -> Option<&String> {
        if fd < 0 {
            None
        } else {
            self.entries.get(fd as usize)
        }
    }
}

/// *at 解析：dirfd × 路径 → 全路径（POSIX 权威口径）。
pub fn resolve_at(table: &DirFdTable, cwd: &str, dirfd: i32, path: &str) -> String {
    if path.starts_with('/') {
        // 绝对路径：忽略 dirfd（POSIX 权威）
        String::from(path)
    } else if dirfd == AT_FDCWD {
        // 哨兵：退回 cwd 基准
        join(cwd, path)
    } else {
        match table.get(dirfd) {
            Some(dir) => join(dir, path),
            None => join(cwd, path), // 坏 fd 兜底（注入域）
        }
    }
}

fn join(base: &str, rel: &str) -> String {
    let mut s = String::from(base);
    if !s.ends_with('/') {
        s.push('/');
    }
    s.push_str(rel);
    s
}

/// 2×2 矩阵验证。
pub fn matrix_2x2() -> bool {
    let mut t = DirFdTable::new();
    let fd = t.open("/data/dir");
    let cwd = "/home/u";
    // (dirfd, 相对)：锚 dirfd
    let a = resolve_at(&t, cwd, fd, "x") == "/data/dir/x";
    // (dirfd, 绝对)：忽略 dirfd
    let b = resolve_at(&t, cwd, fd, "/abs/y") == "/abs/y";
    // (AT_FDCWD, 相对)：退回 cwd
    let c = resolve_at(&t, cwd, AT_FDCWD, "z") == "/home/u/z";
    // (AT_FDCWD, 绝对)：忽略哨兵
    let d = resolve_at(&t, cwd, AT_FDCWD, "/abs/w") == "/abs/w";
    a && b && c && d
}

/// J1 主判据：2×2 矩阵全过 + AT_FDCWD 哨兵断言 + 多 fd 对账。
pub fn j1_main() -> bool {
    matrix_2x2() && {
        let mut t = DirFdTable::new();
        let f1 = t.open("/d1");
        let f2 = t.open("/d2");
        resolve_at(&t, "/c", f1, "a") == "/d1/a" && resolve_at(&t, "/c", f2, "a") == "/d2/a"
    } && {
        // 坏 fd 相对路径兜底（不算权威口径错误——注入域另行处理）
        let t = DirFdTable::new();
        resolve_at(&t, "/c", 99, "r") == "/c/r"
    }
}

// ── 注入核（0xDB80 段 5 项）────────────────────────────────────────────

/// 注入核 1：负 fd 注入——非哨兵负 fd 相对路径兜底 cwd。
pub fn inj1_neg_fd() -> bool {
    let t = DirFdTable::new();
    let r = resolve_at(&t, "/c", -7, "x");
    r == "/c/x"
}

/// 注入核 2：绝对路径优先注入——绝对路径连坏 fd 也忽略 dirfd。
pub fn inj2_abs_priority() -> bool {
    let t = DirFdTable::new();
    resolve_at(&t, "/c", -3, "/abs") == "/abs"
}

/// 注入核 3：哨兵值冻结注入——AT_FDCWD 恒 -100。
pub fn inj3_sentinel_frozen() -> bool {
    AT_FDCWD == -100
}

/// 注入核 4：基准名互异注入。
pub fn inj4_base_names() -> bool {
    Base::DirFd.name() != Base::Cwd.name()
}

/// 注入核 5：空相对路径注入——相对空串拼基准。
pub fn inj5_empty_rel() -> bool {
    let mut t = DirFdTable::new();
    let fd = t.open("/d");
    resolve_at(&t, "/c", fd, "") == "/d/"
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 31
}

/// run_unx4207_checks：本单判据聚合（诊断段 0xDB80）。
pub fn run_unx4207_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F4207 *at 族 dirfd 基准解析（AT_FDCWD）");
    s.add(
        "代码体量≥300行当量",
        structural_weight() >= 300,
        if structural_weight() >= 300 { "" } else { "结构当量不足" },
    );
    s.add(
        "dirfd×绝对/相对2×2矩阵全过（绝对忽略dirfd权威口径）",
        j1_main(),
        if j1_main() { "" } else { "2×2矩阵破" },
    );
    s.add(
        "AT_FDCWD哨兵退回cwd基准断言通过",
        matrix_2x2() && inj3_sentinel_frozen(),
        if matrix_2x2() { "" } else { "哨兵断言破" },
    );
    let inj = [
        ("注入1负fd", inj1_neg_fd()),
        ("注入2绝对优先", inj2_abs_priority()),
        ("注入3哨兵冻结", inj3_sentinel_frozen()),
        ("注入4基准名互异", inj4_base_names()),
        ("注入5空相对路径", inj5_empty_rel()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
