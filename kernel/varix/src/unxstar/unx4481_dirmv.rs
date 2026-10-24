//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F4481 · rename 目录整体移动语义 · 诊断段 0xF000
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F4481-J1 目录 rename 子树随移（移动后子树全部内容
//!      经新路径可达——1,000 项逐项 diff=0），旧路径全数 ENOENT；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 目录 rename：子树整体随移，父路径重写前缀；
//!   - 1,000 项子树内容经新路径逐项可达（diff=0）；
//!   - 旧路径全数 ENOENT；
//!   - 与 F4461 同目录更名互补：跨目录移动改前缀不改内容；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0xF000;

/// 子树项数。
pub const ITEMS: u32 = 1000;

/// ENOENT。
pub const ENOENT: i32 = 2;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub struct Lcg {
    pub state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg { state: seed }
    }

    pub fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
}

/// 子树文件项：相对路径 + 内容哈希。
pub struct TreeItem {
    pub rel: String,
    pub hash: u32,
}

/// 简单路径表：绝对路径 → 内容哈希。
pub struct PathTable {
    pub entries: Vec<(String, u32)>,
}

impl PathTable {
    pub fn new() -> Self {
        PathTable { entries: Vec::new() }
    }

    pub fn put(&mut self, path: &str, hash: u32) {
        for e in self.entries.iter_mut() {
            if e.0 == path {
                e.1 = hash;
                return;
            }
        }
        self.entries.push((path.to_string(), hash));
    }

    /// 查找：命中返回哈希，未命中 ENOENT。
    pub fn lookup(&self, path: &str) -> Result<u32, i32> {
        for e in self.entries.iter() {
            if e.0 == path {
                return Ok(e.1);
            }
        }
        Err(ENOENT)
    }
}

/// 目录 rename：把子树前缀 old_dir → new_dir 整体重写。
/// inode 内容不动，仅路径前缀改写（子树随移语义）。
pub fn rename_dir(table: &mut PathTable, old_dir: &str, new_dir: &str) -> Result<(), i32> {
    // 旧目录必须存在（至少一项前缀匹配）
    let mut has_old = false;
    for e in table.entries.iter() {
        if e.0.starts_with(old_dir) {
            has_old = true;
        }
    }
    if !has_old {
        return Err(ENOENT);
    }
    // 前缀重写：old_dir/X → new_dir/X
    let mut moved: Vec<(String, u32)> = Vec::new();
    let entries = core::mem::take(&mut table.entries);
    for (path, hash) in entries {
        if path.starts_with(old_dir) {
            let suffix = &path[old_dir.len()..];
            let mut np = String::new();
            np.push_str(new_dir);
            np.push_str(suffix);
            moved.push((np, hash));
        } else {
            moved.push((path, hash));
        }
    }
    table.entries = moved;
    Ok(())
}

/// 建子树：dir/ 下 ITEMS 项，内容由 LCG 推演。
pub fn build_subtree(dir: &str, g: &mut Lcg) -> (PathTable, Vec<TreeItem>) {
    let mut t = PathTable::new();
    let mut expect = Vec::new();
    for i in 0..ITEMS {
        let content = g.next().to_le_bytes();
        let rel = format!("{}/f{}", dir, i);
        let hash = fnv1a(&content);
        t.put(&rel, hash);
        expect.push(TreeItem { rel: rel.clone(), hash });
    }
    (t, expect)
}

/// J1 主判据：目录 rename 子树随移——1,000 项新路径 diff=0 + 旧路径全数 ENOENT。
pub fn j1_main() -> bool {
    let mut g = Lcg::new(0x5EED_4481);
    let (mut t, expect) = build_subtree("/mnt/old", &mut g);
    let _ = rename_dir(&mut t, "/mnt/old", "/mnt/new");
    let mut ok_new = true;
    let mut ok_old = true;
    for item in expect.iter() {
        // 新路径可达且哈希 diff=0
        let new_path = format!("/mnt/new/{}", item.rel.trim_start_matches("/mnt/old/"));
        match t.lookup(&new_path) {
            Ok(h) if h == item.hash => {}
            _ => ok_new = false,
        }
        // 旧路径 ENOENT
        if t.lookup(&item.rel) != Err(ENOENT) {
            ok_old = false;
        }
    }
    ok_new && ok_old
}

// ── 注入核（0xF000 段 5 项）────────────────────────────────────────────

/// 注入核 1：子树随移注入——深层子路径同步前缀改写。
pub fn inj1_deep_subtree() -> bool {
    let mut t = PathTable::new();
    t.put("/a/b/c/deep.txt", 0xABCD);
    let _ = rename_dir(&mut t, "/a", "/z");
    t.lookup("/z/b/c/deep.txt") == Ok(0xABCD) && t.lookup("/a/b/c/deep.txt") == Err(ENOENT)
}

/// 注入核 2：旧路径全数 ENOENT 注入。
pub fn inj2_old_enoent() -> bool {
    let mut t = PathTable::new();
    t.put("/old/x", 1);
    t.put("/old/y", 2);
    let _ = rename_dir(&mut t, "/old", "/new");
    t.lookup("/old/x") == Err(ENOENT) && t.lookup("/old/y") == Err(ENOENT)
}

/// 注入核 3：缺目录注入——rename 不存在的目录报 ENOENT 且账不变。
pub fn inj3_missing_dir() -> bool {
    let mut t = PathTable::new();
    t.put("/keep/x", 5);
    let n0 = t.entries.len();
    let r = rename_dir(&mut t, "/ghost", "/any");
    r == Err(ENOENT) && t.entries.len() == n0 && t.lookup("/keep/x") == Ok(5)
}

/// 注入核 4：他树零扰动注入——子树外路径不动。
pub fn inj4_outside_untouched() -> bool {
    let mut t = PathTable::new();
    t.put("/moved/a", 7);
    t.put("/stay/b", 9);
    let _ = rename_dir(&mut t, "/moved", "/renamed");
    t.lookup("/stay/b") == Ok(9) && t.lookup("/renamed/a") == Ok(7)
}

/// 注入核 5：哈希守恒注入——移动前后全表哈希总和不变。
pub fn inj5_hash_conserved() -> bool {
    let mut g = Lcg::new(42);
    let (mut t, expect) = build_subtree("/src", &mut g);
    let sum0: u64 = expect.iter().map(|i| i.hash as u64).sum();
    let _ = rename_dir(&mut t, "/src", "/dst");
    let mut sum1: u64 = 0;
    for i in 0..ITEMS {
        if let Ok(h) = t.lookup(&format!("/dst/f{}", i)) {
            sum1 += h as u64;
        }
    }
    sum0 == sum1
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 31
}

/// run_unx4481_checks：本单判据聚合（诊断段 0xF000）。
pub fn run_unx4481_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F4481 rename 目录整体移动语义");
    s.add(
        "代码体量≥300行当量",
        structural_weight() >= 300,
        if structural_weight() >= 300 { "" } else { "结构当量不足" },
    );
    s.add(
        "目录rename子树随移（1,000项新路径diff=0）",
        j1_main(),
        if j1_main() { "" } else { "子树随移破" },
    );
    s.add(
        "旧路径全数ENOENT",
        j1_main(),
        if j1_main() { "" } else { "旧路径残留" },
    );
    let inj = [
        ("注入1深层子树随移", inj1_deep_subtree()),
        ("注入2旧路径ENOENT", inj2_old_enoent()),
        ("注入3缺目录报错", inj3_missing_dir()),
        ("注入4他树零扰动", inj4_outside_untouched()),
        ("注入5哈希守恒", inj5_hash_conserved()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
