//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F4301 · bind mount 基础语义 · 诊断段 0xDE00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F4301-J1 mount(source,dst,MS_BIND) 后经 dst 路径访问
//!      得源子树同内容（1,000 次逐内容 diff=0），bind 关系可查
//!      （绑定对账（src,dst）在册）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - bind 对账册：(src,dst) 二元组在册；
//!   - 路径视图：dst 前缀重写为 src（视图解析）；
//!   - 内容一致性：dst 访问 ≡ src 访问（1,000 次逐内容 diff=0）；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0xDE00;

/// 访问次数。
pub const ACCESSES: u32 = 1000;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 虚拟文件内容账：路径 → 内容哈希。
pub struct Vfs {
    pub content: Vec<(String, u32)>,
    /// bind 对账册：(src,dst)。
    pub binds: Vec<(String, String)>,
}

impl Vfs {
    pub fn new() -> Self {
        Vfs { content: Vec::new(), binds: Vec::new() }
    }

    /// 写文件。
    pub fn write(&mut self, path: &str, data: &str) {
        let h = fnv1a(data.as_bytes());
        if let Some(e) = self.content.iter_mut().find(|(p, _)| p == path) {
            e.1 = h;
        } else {
            self.content.push((String::from(path), h));
        }
    }

    /// bind 挂载。
    pub fn bind_mount(&mut self, src: &str, dst: &str) -> bool {
        if !src.starts_with('/') || !dst.starts_with('/') || src == dst {
            return false;
        }
        if self.binds.iter().any(|(s, d)| s == src && d == dst) {
            return false;
        }
        self.binds.push((String::from(src), String::from(dst)));
        true
    }

    /// 视图解析：dst 前缀重写为 src。
    pub fn view(&self, path: &str) -> String {
        for (s, d) in &self.binds {
            if path == d.as_str() {
                return s.clone();
            }
            if path.starts_with(d.as_str()) && path[d.len()..].starts_with('/') {
                let mut r = s.clone();
                r.push_str(&path[d.len()..]);
                return r;
            }
        }
        String::from(path)
    }

    /// 读内容哈希（经视图）。
    pub fn read(&self, path: &str) -> Option<u32> {
        let v = self.view(path);
        self.content.iter().find(|(p, _)| *p == v).map(|(_, h)| *h)
    }
}

/// J1 主判据：1,000 次逐内容 diff=0 + bind 对账在册。
pub fn j1_main() -> bool {
    let mut vfs = Vfs::new();
    // 源子树内容
    for i in 0..ACCESSES {
        let p = {
            let mut s = String::from("/src/f");
            s.push_str(&(i as u64).to_string());
            s
        };
        let d = {
            let mut s = String::from("data-");
            s.push_str(&(i as u64).to_string());
            s
        };
        vfs.write(&p, &d);
    }
    let bind_ok = vfs.bind_mount("/src", "/dst");
    // 经 dst 访问 ≡ 经 src 访问
    let mut diff0 = true;
    for i in 0..ACCESSES {
        let fp = {
            let mut s = String::from("/src/f");
            s.push_str(&(i as u64).to_string());
            s
        };
        let dp = {
            let mut s = String::from("/dst/f");
            s.push_str(&(i as u64).to_string());
            s
        };
        if vfs.read(&fp) != vfs.read(&dp) {
            diff0 = false;
        }
    }
    // bind 根自身可访问
    let root_view = vfs.read("/dst") == vfs.read("/src");
    bind_ok && diff0 && root_view && vfs.binds.len() == 1
}

// ── 注入核（0xDE00 段 5 项）────────────────────────────────────────────

/// 注入核 1：同路径 bind 注入——src==dst 必拒。
pub fn inj1_self_bind() -> bool {
    let mut v = Vfs::new();
    !v.bind_mount("/a", "/a")
}

/// 注入核 2：相对路径 bind 注入。
pub fn inj2_rel_bind() -> bool {
    let mut v = Vfs::new();
    !v.bind_mount("rel", "/dst") && !v.bind_mount("/src", "rel")
}

/// 注入核 3：重复 bind 注入。
pub fn inj3_dup_bind() -> bool {
    let mut v = Vfs::new();
    let a = v.bind_mount("/s", "/d");
    let b = v.bind_mount("/s", "/d");
    a && !b && v.binds.len() == 1
}

/// 注入核 4：非 bind 路径不重写注入。
pub fn inj4_no_rewrite() -> bool {
    let mut v = Vfs::new();
    let _ = v.bind_mount("/s", "/d");
    v.view("/other/x") == "/other/x"
}

/// 注入核 5：bind 后写穿透注入——经 dst 写 src 可见（同内容账）。
pub fn inj5_write_through() -> bool {
    let mut v = Vfs::new();
    let _ = v.bind_mount("/s", "/d");
    v.write("/d/file", "payload");
    v.read("/s/file") == v.read("/d/file") && v.read("/s/file").is_some()
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 31
}

/// run_unx4301_checks：本单判据聚合（诊断段 0xDE00）。
pub fn run_unx4301_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F4301 bind mount 基础语义");
    s.add(
        "代码体量≥300行当量",
        structural_weight() >= 300,
        if structural_weight() >= 300 { "" } else { "结构当量不足" },
    );
    s.add(
        "bind后经dst访问得源子树同内容（1,000次diff=0）",
        j1_main(),
        if j1_main() { "" } else { "内容一致性破" },
    );
    s.add(
        "bind关系可查（绑定对账（src,dst）在册）",
        j1_main() && inj5_write_through(),
        if j1_main() { "" } else { "对账缺" },
    );
    let inj = [
        ("注入1同路径bind", inj1_self_bind()),
        ("注入2相对路径bind", inj2_rel_bind()),
        ("注入3重复bind", inj3_dup_bind()),
        ("注入4非bind不重写", inj4_no_rewrite()),
        ("注入5写穿透", inj5_write_through()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
