//! UNX-F46431 sysroot 完整性校验器（目录树哈希账 + 变更即红）（0x401E0 段）
//!
//! 锚点：UNX-F46431-J1 树哈希 ×2 次一致、单文件篡改注入 ×10 必检出并
//! 定位到路径。
//! 域：CoRun·UNX-S1-B02（工具链域·完整性校验面）。
//!
//! 结构：sysroot 布点清单（usr/include × usr/lib × lib 三族路径）+ 文件
//! 节点（路径 × 内容指纹）+ 目录树账（路径字典序全序 + 树哈希 = 全节点
//! FNV 折叠）+ 双算一致核（同账两次哈希必等）+ 篡改定位核（新旧账逐路
//! 径比对，漂移路径显性化）+ 注入核（×10 篡改逐次检出 + 重复路径/越布
//! 点/封账改写/空树哈希必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x401E0 独占 0x10）。
pub const SEG_BASE: u32 = 0x401E0;
pub const TH_DUP: u32 = SEG_BASE + 0x01; // 重复路径
pub const TH_OOB: u32 = SEG_BASE + 0x02; // 越布点清单
pub const TH_SEALED: u32 = SEG_BASE + 0x03; // 封账改写
pub const TH_EMPTY: u32 = SEG_BASE + 0x04; // 空树哈希拒
pub const TH_TAMPER: u32 = SEG_BASE + 0x05; // 篡改未检出

/// sysroot 布点清单 ×20（三族：头/静态库/动态库）。
pub const TH_PATHS: [&str; 20] = [
    "usr/include/stdio.h",
    "usr/include/stdlib.h",
    "usr/include/string.h",
    "usr/include/stdint.h",
    "usr/include/stddef.h",
    "usr/include/unistd.h",
    "usr/include/fcntl.h",
    "usr/include/errno.h",
    "usr/lib/libc.a",
    "usr/lib/libm.a",
    "usr/lib/libgcc.a",
    "usr/lib/libstdc++.a",
    "usr/lib/crt0.o",
    "usr/lib/crti.o",
    "usr/lib/crtn.o",
    "lib/libc.so",
    "lib/libm.so",
    "lib/ld-so",
    "lib/libpthread.so",
    "lib/libdl.so",
];

/// 路径族判定：0 = 头族，1 = 静态库/启动件族，2 = 动态库族，其他 = Err。
pub fn path_family(p: &str) -> Result<u8, u32> {
    if p.starts_with("usr/include/") {
        Ok(0)
    } else if p.starts_with("usr/lib/") {
        Ok(1)
    } else if p.starts_with("lib/") {
        Ok(2)
    } else {
        Err(TH_OOB)
    }
}

/// 文件节点：路径 × 内容指纹。
#[derive(Clone)]
pub struct FileNode {
    pub path: String,
    pub fp: u32,
}

/// 确定性内容指纹（FNV-1a）。
pub fn content_fp(path: &str, content: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for b in path.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(16777619);
    }
    for b in content {
        h ^= *b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 目录树账：路径字典序全序存放。
pub struct TreeBook {
    pub files: Vec<FileNode>,
    pub sealed: bool,
}

impl TreeBook {
    pub fn new() -> Self {
        TreeBook { files: Vec::new(), sealed: false }
    }

    /// 登记一个布点（重复/越布点/封账拒绝；登记后保持字典序）。
    pub fn put(&mut self, path: &str, content: &[u8]) -> Result<(), u32> {
        if self.sealed {
            return Err(TH_SEALED);
        }
        if !TH_PATHS.contains(&path) {
            return Err(TH_OOB);
        }
        if self.files.iter().any(|f| f.path == path) {
            return Err(TH_DUP);
        }
        let node = FileNode { path: path.to_string(), fp: content_fp(path, content) };
        let pos = self
            .files
            .binary_search_by(|f| f.path.as_str().cmp(path))
            .unwrap_or_else(|p| p);
        self.files.insert(pos, node);
        Ok(())
    }

    /// 查询节点指纹。
    pub fn get(&self, path: &str) -> Option<u32> {
        self.files.iter().find(|f| f.path == path).map(|f| f.fp)
    }

    /// 树满核（20 布点全登记）。
    pub fn full(&self) -> bool {
        self.files.len() == TH_PATHS.len()
    }

    /// 字典序核（登记序即全序）。
    pub fn sorted(&self) -> bool {
        self.files.windows(2).all(|w| w[0].path < w[1].path)
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 树哈希：全节点（路径 × 指纹）按字典序 FNV 折叠。空树拒绝。
    pub fn tree_hash(&self) -> Result<u32, u32> {
        if self.files.is_empty() {
            return Err(TH_EMPTY);
        }
        let mut h: u32 = 2166136261;
        for f in self.files.iter() {
            for b in format!("{}|{:08x};", f.path, f.fp).as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(16777619);
            }
        }
        Ok(h)
    }

    /// 篡改定位：与本账逐路径比对新账，返回指纹漂移的路径集（字典序）。
    pub fn locate_drift(&self, other: &TreeBook) -> Vec<String> {
        let mut out = Vec::new();
        for f in self.files.iter() {
            match other.get(&f.path) {
                None => out.push(f.path.clone()), // 新账缺点 = 漂移
                Some(nfp) if nfp != f.fp => out.push(f.path.clone()),
                _ => {}
            }
        }
        for f in other.files.iter() {
            if self.get(&f.path).is_none() {
                let pos = out
                    .binary_search_by(|p| p.as_str().cmp(f.path.as_str()))
                    .unwrap_or_else(|p| p);
                out.insert(pos, f.path.clone());
            }
        }
        out
    }
}

/// 族计数审计：三族各登记多少布点（满账应为 8/7/5）。
pub fn family_counts(book: &TreeBook) -> [usize; 3] {
    let mut c = [0usize; 3];
    for f in book.files.iter() {
        if let Ok(k) = path_family(&f.path) {
            if (k as usize) < 3 {
                c[k as usize] += 1;
            }
        }
    }
    c
}

/// 漂移报告渲染：逐漂移路径 × 族标签 × 新旧指纹对（可导出文本）。
pub fn render_diff(base: &TreeBook, other: &TreeBook) -> String {
    let mut out = String::new();
    out.push_str("== UNX-F46431 完整性漂移报告 ==\n");
    let drift = base.locate_drift(other);
    for p in drift.iter() {
        let fam = path_family(p).unwrap_or(255);
        let tag = match fam {
            0 => "INC",
            1 => "SLIB",
            2 => "DLIB",
            _ => "??",
        };
        let old_fp = base.get(p).unwrap_or(0);
        let new_fp = other.get(p).unwrap_or(0);
        out.push_str(&format!("- [{}] {} {:08x}->{:08x}\n", tag, p, old_fp, new_fp));
    }
    out.push_str(&format!("drift_n={}\n", drift.len()));
    out
}

/// 标准布点内容（确定性：内容 = 路径派生字节流）。
pub fn fixture_content(path: &str) -> Vec<u8> {
    format!("/* sysroot fixture: {} */\n", path).into_bytes()
}

/// 标准账夹具：20 布点全登记 + 封账。
pub fn standard_book() -> TreeBook {
    let mut b = TreeBook::new();
    for p in TH_PATHS.iter() {
        b.put(p, &fixture_content(p)).expect("fixture put");
    }
    b.seal();
    b
}

/// 用内容表（路径 → 新内容）建未封账新账。
pub fn rebuild(contents: &[(String, Vec<u8>)]) -> TreeBook {
    let mut b = TreeBook::new();
    for (p, c) in contents.iter() {
        b.put(p, c).expect("rebuild put");
    }
    b
}

/// 篡改注入核：×10 逐次单文件篡改 → 必检出且定位路径正确。
pub fn tamper_core() -> (usize, usize) {
    let base = standard_book();
    let mut caught = 0usize;
    for k in 0..10usize {
        let target = TH_PATHS[k * 2]; // 每 2 布点取 1，覆盖三族
        let mut contents: Vec<(String, Vec<u8>)> = TH_PATHS
            .iter()
            .map(|p| ((*p).to_string(), fixture_content(p)))
            .collect();
        for (p, c) in contents.iter_mut() {
            if p == target {
                c.push(b'#'); // 单文件篡改
            }
        }
        let other = rebuild(&contents);
        let drift = base.locate_drift(&other);
        let hash_changed = base.tree_hash() != other.tree_hash();
        if drift.len() == 1 && drift[0] == target && hash_changed {
            caught += 1;
        }
    }
    (caught, 10)
}

/// 注入核：守卫面（重复/越布点/封账改写/空树哈希/篡改核）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 重复路径必抓。
    let mut b1 = TreeBook::new();
    b1.put("usr/include/stdio.h", b"a").expect("first");
    if b1.put("usr/include/stdio.h", b"b") == Err(TH_DUP) {
        caught += 1;
    }
    // 2) 越布点清单必抓（三族外路径）。
    let mut b2 = TreeBook::new();
    if b2.put("etc/passwd", b"x") == Err(TH_OOB) && path_family("etc/passwd") == Err(TH_OOB) {
        caught += 1;
    }
    // 3) 封账改写必抓。
    let mut b3 = standard_book();
    b3.seal();
    if b3.put("usr/include/stdio.h", b"y") == Err(TH_SEALED) {
        caught += 1;
    }
    // 4) 空树哈希拒。
    let b4 = TreeBook::new();
    if b4.tree_hash() == Err(TH_EMPTY) {
        caught += 1;
    }
    // 5) 双算一致核（同账两次哈希必等 + 字典序核）。
    let b5 = standard_book();
    let h1 = b5.tree_hash().unwrap_or(0);
    let h2 = b5.tree_hash().unwrap_or(1);
    if h1 == h2 && b5.sorted() && b5.full() {
        caught += 1;
    }
    // 6) 篡改核 ×10 全检出。
    let (c, n) = tamper_core();
    if c == n {
        caught += 1;
    }
    // 7) 无篡改时定位零漂移。
    let base = standard_book();
    let same = standard_book();
    if base.locate_drift(&same).is_empty() && base.tree_hash() == same.tree_hash() {
        caught += 1;
    }
    (caught, 7)
}

/// 判据入口（CheckSet）。
pub fn run_unx46431_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F46431");
    // J1-1 树哈希 ×2 次一致。
    let book = standard_book();
    let h1 = book.tree_hash().unwrap_or(0);
    let h2 = book.tree_hash().unwrap_or(1);
    let ok = book.full() && h1 == h2 && h1 != 0;
    s.add("46431-J1-双算一致", ok, if ok { "" } else { "双算漂移" });
    // J1-2 单文件篡改注入 ×10 必检出并定位。
    let (c, n) = tamper_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("篡改检出 {c}/{n}") };
    s.add_dyn("46431-J1-篡改定位", ok, &detail);
    // J1-3 三族路径族判定全对（20 布点逐个）。
    let ok = TH_PATHS.iter().enumerate().all(|(i, p)| {
        let want = if i < 8 { 0 } else if i < 15 { 1 } else { 2 };
        path_family(p) == Ok(want)
    });
    s.add("46431-J1-路径族", ok, if ok { "" } else { "路径族漂移" });
    // J1-4 字典序登记 + 全点覆盖。
    let ok = book.sorted() && book.files.len() == 20
        && book.files.first().map(|f| f.path.as_str()) == Some("lib/ld-so")
        && book.files.last().map(|f| f.path.as_str()) == Some("usr/lib/libstdc++.a");
    s.add("46431-J1-字典序", ok, if ok { "" } else { "字典序漂移" });
    // J1-5 变更即红：任一单点漂移 → 树哈希必变。
    let mut contents: Vec<(String, Vec<u8>)> =
        TH_PATHS.iter().map(|p| ((*p).to_string(), fixture_content(p))).collect();
    contents[17].1.push(b'!'); // lib/ld-so 篡改
    let other = rebuild(&contents);
    let drift = book.locate_drift(&other);
    let ok = book.tree_hash() != other.tree_hash() && drift == vec!["lib/ld-so".to_string()];
    s.add("46431-J1-变更即红", ok, if ok { "" } else { "变更即红失灵" });
    // J1-6 布点指纹查询一致。
    let ok = TH_PATHS.iter().all(|p| {
        book.get(p) == Some(content_fp(p, &fixture_content(p)))
    });
    s.add("46431-J1-指纹查询", ok, if ok { "" } else { "指纹漂移" });
    // J1-7 族计数满账 8/7/5 + 漂移报告渲染逐字核对。
    let fc = family_counts(&book);
    let mut contents: Vec<(String, Vec<u8>)> =
        TH_PATHS.iter().map(|p| ((*p).to_string(), fixture_content(p))).collect();
    contents[0].1.push(b'@'); // 头族 1 点篡改
    contents[15].1.push(b'@'); // 动态库族 1 点篡改
    let other = rebuild(&contents);
    let rep = render_diff(&book, &other);
    let ok = fc == [8, 7, 5]
        && rep.contains("[INC] usr/include/stdio.h")
        && rep.contains("[DLIB] lib/libc.so")
        && rep.contains("drift_n=2")
        && rep.contains("== UNX-F46431 完整性漂移报告 ==");
    s.add("46431-J1-族计数与报告", ok, if ok { "" } else { "族计数/报告漂移" });
    // 注入核 7/7。
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("46431-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx46431_all_checks_green() {
        let set = run_unx46431_checks();
        assert!(set.all_passed(), "UNX-F46431 checks must be green: {}", set.red_items_text());
    }
}
