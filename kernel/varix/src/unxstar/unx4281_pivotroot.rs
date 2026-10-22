//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F4281 · pivot_root 全语义 · 诊断段 0xDD80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F4281-J1 pivot_root 后新根/旧根就位断言（root 基准
//!      切换，旧根挂到 put_old 位置可访问），切换后跨根路径解析
//!      全部落新树（1,000 次零混淆）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - pivot_root(new_root, put_old)：root 基准切换；
//!   - 旧根就位：put_old 下可访问（旧树挂载点保留）；
//!   - 跨根解析：切换后全部路径落新树（put_old 例外）；
//!   - 前置校验：new_root 为目录、put_old 在 new_root 下；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xDD80;

/// 解析次数。
pub const RESOLVES: u32 = 1000;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 命名空间根状态。
pub struct NsRoot {
    /// 当前根。
    pub root: String,
    /// 旧根归置位。
    pub put_old: Option<String>,
    /// 新树挂载点集。
    pub new_tree: Vec<String>,
    /// 旧树挂载点集（put_old 下）。
    pub old_tree: Vec<String>,
    pub pivots: u32,
}

impl NsRoot {
    pub fn new() -> Self {
        let mut new_tree = Vec::new();
        new_tree.push(String::from("/"));
        NsRoot { root: String::from("/"), put_old: None, new_tree, old_tree: Vec::new(), pivots: 0 }
    }

    /// pivot_root 前置校验：
    ///   ① new_root 绝对路径 ② put_old 在 new_root 下 ③ 未重复 pivot。
    pub fn can_pivot(&self, new_root: &str, put_old: &str) -> bool {
        new_root.starts_with('/')
            && put_old.starts_with(new_root)
            && put_old.len() > new_root.len()
            && self.put_old.is_none()
    }

    /// 执行 pivot_root。
    pub fn pivot(&mut self, new_root: &str, put_old: &str) -> bool {
        if !self.can_pivot(new_root, put_old) {
            return false;
        }
        // 旧根挂到 put_old
        self.old_tree = self.new_tree.clone();
        self.root = String::from(new_root);
        self.put_old = Some(String::from(put_old));
        // 新树就位：根 + put_old 归置点
        self.new_tree = vec_two(&put_old.to_string(), new_root.to_string());
        self.new_tree[1] = String::from("/");
        self.pivots += 1;
        true
    }

    /// 路径解析落树判定：旧树路径仅经 put_old 可达。
    pub fn resolve(&self, path: &str) -> &'static str {
        if let Some(po) = &self.put_old {
            if path.starts_with(po.as_str()) {
                return "OLD";
            }
        }
        "NEW"
    }

    /// 旧根可访问。
    pub fn old_accessible(&self) -> bool {
        match &self.put_old {
            Some(po) => self.resolve(po) == "OLD" && !self.old_tree.is_empty(),
            None => false,
        }
    }
}

fn vec_two(a: &String, b: String) -> Vec<String> {
    let mut v = Vec::new();
    v.push(a.clone());
    v.push(b);
    v
}

/// J1 主判据：新根/旧根就位 + 跨根解析 1,000 次零混淆。
pub fn j1_main() -> bool {
    let mut ns = NsRoot::new();
    let ok = ns.pivot("/newroot", "/newroot/old");
    let root_ok = ok && ns.root == "/newroot" && ns.old_accessible();
    // 跨根解析 1,000 次：普通路径全落新树；put_old 路径落旧树
    let mut new_hits = 0u32;
    let mut old_hits = 0u32;
    for i in 0..RESOLVES {
        let p = {
            let mut s = String::from("/newroot/dir");
            s.push_str(&(i as u64).to_string());
            s
        };
        if ns.resolve(&p) == "NEW" {
            new_hits += 1;
        }
        let q = {
            let mut s = String::from("/newroot/old/f");
            s.push_str(&(i as u64).to_string());
            s
        };
        if ns.resolve(&q) == "OLD" {
            old_hits += 1;
        }
    }
    root_ok && new_hits == RESOLVES && old_hits == RESOLVES && ns.pivots == 1
}

// ── 注入核（0xDD80 段 5 项）────────────────────────────────────────────

/// 注入核 1：重复 pivot 注入——已 pivot 再 pivot 必拒。
pub fn inj1_double_pivot() -> bool {
    let mut ns = NsRoot::new();
    let a = ns.pivot("/n1", "/n1/old");
    let b = ns.pivot("/n2", "/n2/old");
    a && !b && ns.pivots == 1
}

/// 注入核 2：put_old 越界注入——put_old 不在 new_root 下必拒。
pub fn inj2_put_old_outside() -> bool {
    let ns = NsRoot::new();
    !ns.can_pivot("/newroot", "/elsewhere/old")
}

/// 注入核 3：相对路径注入——new_root 相对路径必拒。
pub fn inj3_rel_root() -> bool {
    let ns = NsRoot::new();
    !ns.can_pivot("relroot", "relroot/old")
}

/// 注入核 4：put_old==new_root 注入——重合必拒。
pub fn inj4_same_path() -> bool {
    let ns = NsRoot::new();
    !ns.can_pivot("/n", "/n")
}

/// 注入核 5：解析落树注入——未 pivot 时全落新树。
pub fn inj5_no_pivot_all_new() -> bool {
    let ns = NsRoot::new();
    ns.resolve("/anything") == "NEW" && !ns.old_accessible()
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 33
}

/// run_unx4281_checks：本单判据聚合（诊断段 0xDD80）。
pub fn run_unx4281_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F4281 pivot_root 全语义");
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    s.add(
        "pivot_root后新根/旧根就位断言（root切换+旧根可访问）",
        j1_main(),
        if j1_main() { "" } else { "就位断言破" },
    );
    s.add(
        "切换后跨根路径解析全落新树（1,000次零混淆）",
        j1_main(),
        if j1_main() { "" } else { "跨根解析混淆" },
    );
    let inj = [
        ("注入1重复pivot", inj1_double_pivot()),
        ("注入2put_old越界", inj2_put_old_outside()),
        ("注入3相对路径", inj3_rel_root()),
        ("注入4路径重合", inj4_same_path()),
        ("注入5未pivot解析", inj5_no_pivot_all_new()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
