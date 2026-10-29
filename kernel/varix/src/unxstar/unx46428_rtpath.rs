//! UNX-F46428 运行时库落位账（libgcc/libstdc++ 类与 gcc 目录族对应）（0x401B0 段）
//!
//! 锚点：UNX-F46428-J1 运行时库 ×6 逐个落位一致、重复库双版本共存量零。
//! 域：CoRun·UNX-S1-B02（工具链域·运行时落位面）。
//!
//! 结构：运行时库六件账（libgcc.a/libgcc_eh.a/libgcc_s.so/libstdc++.a/
//! libstdc++.so/libsupc++.a 逐件登记：库名 × 落位目录 × 版本 × 静动态
//! 类别）+ gcc 目录族对应核（库目录 ⊆ gcc 目录族）+ 双版本共存检测
//! （同库同目录两版本 = 污染必抓）+ 注入核（缺件/目录漂移/双版本/
//! 越类别/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x401B0 独占 0x10）。
pub const SEG_BASE: u32 = 0x401B0;
pub const RT_MISS: u32 = SEG_BASE + 0x01; // 库缺件
pub const RT_DIR_DRIFT: u32 = SEG_BASE + 0x02; // 目录漂移
pub const RT_DUAL_VERSION: u32 = SEG_BASE + 0x03; // 双版本共存
pub const RT_KIND_OOB: u32 = SEG_BASE + 0x04; // 类别越闭集
pub const RT_SEALED: u32 = SEG_BASE + 0x05; // 封账改写

/// gcc 目录族（库落位候选目录）。
pub const DIR_GCC_TRIPLE: &str = "/usr/lib/gcc/x86_64-varix-unxreal/13.2.0";
pub const DIR_USR_LIB: &str = "/usr/lib/x86_64-varix-unxreal";
pub const GCC_DIR_FAMILY: [&str; 2] = [DIR_GCC_TRIPLE, DIR_USR_LIB];

/// 运行时库六件闭集。
pub const L_GCC_A: usize = 0;
pub const L_GCC_EH: usize = 1;
pub const L_GCC_S: usize = 2;
pub const L_STDCXX_A: usize = 3;
pub const L_STDCXX_SO: usize = 4;
pub const L_SUPCXX: usize = 5;
pub const L_COUNT: usize = 6;

pub const L_NAMES: [&str; L_COUNT] = [
    "libgcc.a", "libgcc_eh.a", "libgcc_s.so", "libstdc++.a", "libstdc++.so", "libsupc++.a",
];

/// 库类别：静态 / 动态。
pub const KIND_STATIC: u8 = 0;
pub const KIND_SHARED: u8 = 1;

/// 库账目：名 × 目录 × 版本 × 类别。
#[derive(Clone)]
pub struct RtEntry {
    pub lib: usize,
    pub dir: String,
    pub version: String,
    pub kind: u8,
}

impl RtEntry {
    /// 类别与扩展名一致核（.a=静态，.so=动态）。
    pub fn kind_matches_name(&self) -> bool {
        let name = L_NAMES[self.lib];
        if name.ends_with(".a") {
            self.kind == KIND_STATIC
        } else if name.ends_with(".so") {
            self.kind == KIND_SHARED
        } else {
            false
        }
    }

    /// 全路径。
    pub fn full_path(&self) -> String {
        format!("{}/{}", self.dir, L_NAMES[self.lib])
    }

    /// 账目指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for b in format!("{}|{}|{}|{};", self.lib, self.dir, self.version, self.kind).as_bytes() {
            h ^= *b as u32;
            h = h.wrapping_mul(16777619);
        }
        h
    }
}

/// 运行时落位账（六件逐件唯一）。
pub struct RtBook {
    pub entries: Vec<RtEntry>,
    pub sealed: bool,
}

impl RtBook {
    pub fn new() -> Self {
        RtBook { entries: Vec::new(), sealed: false }
    }

    /// 登记项（越件/越类别/目录越族/重复/封账全拒）。
    pub fn put(&mut self, lib: usize, dir: &str, version: &str, kind: u8) -> Result<(), u32> {
        if lib >= L_COUNT {
            return Err(RT_MISS);
        }
        if kind > KIND_SHARED {
            return Err(RT_KIND_OOB);
        }
        if !GCC_DIR_FAMILY.iter().any(|d| *d == dir) {
            return Err(RT_DIR_DRIFT);
        }
        if version.is_empty() {
            return Err(RT_DIR_DRIFT);
        }
        if self.sealed {
            return Err(RT_SEALED);
        }
        if self.entries.iter().any(|e| e.lib == lib) {
            return Err(RT_DUAL_VERSION);
        }
        let e = RtEntry {
            lib,
            dir: dir.to_string(),
            version: version.to_string(),
            kind,
        };
        if !e.kind_matches_name() {
            return Err(RT_KIND_OOB);
        }
        self.entries.push(e);
        Ok(())
    }

    /// ×6 全登记核。
    pub fn full6(&self) -> bool {
        self.entries.len() == L_COUNT && (0..L_COUNT).all(|l| self.entries.iter().any(|e| e.lib == l))
    }

    /// 双版本共存检测：同库名同目录出现两版本（账内同 lib 只一条，此核
    /// 对「外部目录清单」做检测——模拟扫描结果注入）。
    pub fn dual_version_scan(dirs: &[(String, String)]) -> Vec<String> {
        // dirs: (库文件名, 版本段)。同名库出现两个不同版本段 = 共存。
        let mut out = Vec::new();
        for i in 0..dirs.len() {
            for j in (i + 1)..dirs.len() {
                if dirs[i].0 == dirs[j].0 && dirs[i].1 != dirs[j].1 {
                    out.push(format!("{}|{}|{}", dirs[i].0, dirs[i].1, dirs[j].1));
                }
            }
        }
        out
    }

    /// gcc 目录族对应核：每件目录 ∈ 目录族。
    pub fn dirs_in_family(&self) -> bool {
        self.entries
            .iter()
            .all(|e| GCC_DIR_FAMILY.iter().any(|d| *d == e.dir))
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 账指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for e in self.entries.iter() {
            h ^= e.fingerprint();
            h = h.wrapping_mul(16777619);
        }
        h
    }
}

/// 标准夹具：六件全登记（gcc 目录族两目录分摊）。
pub fn standard_book() -> RtBook {
    let mut b = RtBook::new();
    // libgcc 三件 → gcc/triple 目录（编译器私有运行时）。
    b.put(L_GCC_A, DIR_GCC_TRIPLE, "13.2.0", KIND_STATIC).expect("gcc.a");
    b.put(L_GCC_EH, DIR_GCC_TRIPLE, "13.2.0", KIND_STATIC).expect("gcc_eh");
    b.put(L_GCC_S, DIR_GCC_TRIPLE, "13.2.0", KIND_SHARED).expect("gcc_s");
    // libstdc++ 三件 → usr/lib（目标系统库）。
    b.put(L_STDCXX_A, DIR_USR_LIB, "13.2.0", KIND_STATIC).expect("stdc++.a");
    b.put(L_STDCXX_SO, DIR_USR_LIB, "13.2.0", KIND_SHARED).expect("stdc++.so");
    b.put(L_SUPCXX, DIR_USR_LIB, "13.2.0", KIND_STATIC).expect("supc++");
    b
}

/// 落位渲染（库|全路径|类别 六行）。
pub fn render_rt(b: &RtBook) -> String {
    let mut out = String::new();
    for e in b.entries.iter() {
        out.push_str(&format!(
            "{}|{}|{}\n",
            L_NAMES[e.lib],
            e.full_path(),
            if e.kind == KIND_STATIC { "static" } else { "shared" }
        ));
    }
    out
}

/// 渲染完整性核（六行 × 三段，类别列闭合）。
pub fn render_rt_ok(b: &RtBook) -> bool {
    let text = render_rt(b);
    let lines: Vec<&str> = text.lines().collect();
    lines.len() == 6
        && lines.iter().all(|ln| {
            let seg: Vec<&str> = ln.split('|').collect();
            seg.len() == 3 && (seg[2] == "static" || seg[2] == "shared") && seg[1].contains('/')
        })
}

/// 目录分摊核（gcc 私有三件在 gcc 目录、目标系统三件在 usr/lib）。
pub fn dir_split_ok(b: &RtBook) -> bool {
    let gcc_private = [L_GCC_A, L_GCC_EH, L_GCC_S];
    let target_sys = [L_STDCXX_A, L_STDCXX_SO, L_SUPCXX];
    gcc_private.iter().all(|l| {
        b.entries
            .iter()
            .find(|e| e.lib == *l)
            .map(|e| e.dir == DIR_GCC_TRIPLE)
            .unwrap_or(false)
    }) && target_sys.iter().all(|l| {
        b.entries
            .iter()
            .find(|e| e.lib == *l)
            .map(|e| e.dir == DIR_USR_LIB)
            .unwrap_or(false)
    })
}

/// 版本一致性核：六件版本全同（运行时族版本纪律）。
pub fn version_uniform(b: &RtBook) -> bool {
    if b.entries.len() != L_COUNT {
        return false;
    }
    let v0 = &b.entries[0].version;
    b.entries.iter().all(|e| &e.version == v0)
}

/// 静动态配对核：静态件与动态件成对（libgcc.a↔libgcc_s.so、libstdc++.a↔libstdc++.so）。
pub fn static_shared_pairs_ok(b: &RtBook) -> bool {
    let pairs: [(usize, usize); 2] = [(L_GCC_A, L_GCC_S), (L_STDCXX_A, L_STDCXX_SO)];
    pairs.iter().all(|(a, b2)| {
        b.entries.iter().any(|e| e.lib == *a && e.kind == KIND_STATIC)
            && b.entries.iter().any(|e| e.lib == *b2 && e.kind == KIND_SHARED)
    })
}

/// soname 语义：动态件 soname = 库名.主版（libgcc_s.so.13 形态）。
pub fn soname_of(b: &RtBook, lib: usize) -> Result<String, u32> {
    match b.entries.iter().find(|e| e.lib == lib) {
        None => Err(RT_MISS),
        Some(e) => {
            if e.kind != KIND_SHARED {
                return Err(RT_KIND_OOB);
            }
            let major = e.version.split('.').next().unwrap_or("0");
            Ok(format!("{}.{}", L_NAMES[lib], major))
        }
    }
}

/// soname 核（两动态件推导形态正确）。
pub fn sonames_ok(b: &RtBook) -> bool {
    soname_of(b, L_GCC_S) == Ok("libgcc_s.so.13".to_string())
        && soname_of(b, L_STDCXX_SO) == Ok("libstdc++.so.13".to_string())
        && soname_of(b, L_GCC_A) == Err(RT_KIND_OOB)
}

/// 库账导出 schema 完整率核（六件名逐个在渲染文中）。
pub fn export_coverage_ok(b: &RtBook) -> bool {
    let text = render_rt(b);
    L_NAMES.iter().all(|n| text.contains(n))
}

/// 目录族常量核（两目录非空且互异）。
pub fn dir_family_ok() -> bool {
    GCC_DIR_FAMILY.len() == 2
        && GCC_DIR_FAMILY[0] != GCC_DIR_FAMILY[1]
        && GCC_DIR_FAMILY.iter().all(|d| d.starts_with("/usr"))
}

/// 全路径在位核（六件全路径逐个以目录族为前缀且以库名收尾）。
pub fn full_paths_ok(b: &RtBook) -> bool {
    b.entries.iter().all(|e| {
        let p = e.full_path();
        GCC_DIR_FAMILY.iter().any(|d| p.starts_with(d)) && p.ends_with(L_NAMES[e.lib])
    })
}

/// 目录族登记序守卫：三件 gcc 私有 + 三件目标系统 = 六件守恒。
pub fn dir_split_counts(b: &RtBook) -> (usize, usize) {
    let in_gcc = b.entries.iter().filter(|e| e.dir == DIR_GCC_TRIPLE).count();
    let in_usr = b.entries.iter().filter(|e| e.dir == DIR_USR_LIB).count();
    (in_gcc, in_usr)
}
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 缺件必抓（五件账）
    let mut b = RtBook::new();
    for l in 0..5 {
        let dir = if l < 3 { DIR_GCC_TRIPLE } else { DIR_USR_LIB };
        let kind = if l == 2 || l == 4 { KIND_SHARED } else { KIND_STATIC };
        b.put(l, dir, "13.2.0", kind).expect("partial");
    }
    if !b.full6() {
        caught += 1;
    }
    // 2) 目录越族必抓
    let mut b2 = RtBook::new();
    if b2.put(L_GCC_A, "/opt/elsewhere", "13.2.0", KIND_STATIC) == Err(RT_DIR_DRIFT) {
        caught += 1;
    }
    // 3) 双版本共存必抓（扫描注入）
    let scan = vec![
        ("libgcc_s.so".to_string(), "13.2.0".to_string()),
        ("libgcc_s.so".to_string(), "12.1.0".to_string()),
    ];
    let dual = RtBook::dual_version_scan(&scan);
    if dual.len() == 1 {
        caught += 1;
    }
    // 4) 类别与扩展名矛盾必抓（.a 登记为 shared）
    let mut b3 = RtBook::new();
    if b3.put(L_GCC_A, DIR_GCC_TRIPLE, "13.2.0", KIND_SHARED) == Err(RT_KIND_OOB) {
        caught += 1;
    }
    // 5) 封账改写必抓
    let mut std_b = standard_book();
    std_b.seal();
    if std_b.put(L_GCC_A, DIR_GCC_TRIPLE, "13.2.0", KIND_STATIC) == Err(RT_SEALED) {
        caught += 1;
    }
    // 6) 重复登记必抓
    let mut b4 = RtBook::new();
    b4.put(L_GCC_A, DIR_GCC_TRIPLE, "13.2.0", KIND_STATIC).expect("first");
    if b4.put(L_GCC_A, DIR_USR_LIB, "13.2.0", KIND_STATIC) == Err(RT_DUAL_VERSION) {
        caught += 1;
    }
    (caught, 6)
}

/// 判据入口（CheckSet）。
pub fn run_unx46428_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F46428");
    // J1-1 运行时库 ×6 逐个落位一致
    let b = standard_book();
    let ok = b.full6() && b.dirs_in_family() && dir_split_ok(&b);
    s.add("46428-J1-×6落位一致", ok, if ok { "" } else { "落位漂移" });
    // J1-2 重复库双版本共存量零（标准目录清单扫描）
    let clean_scan = vec![
        ("libgcc.a".to_string(), "13.2.0".to_string()),
        ("libgcc_s.so".to_string(), "13.2.0".to_string()),
        ("libstdc++.so".to_string(), "13.2.0".to_string()),
    ];
    let ok = RtBook::dual_version_scan(&clean_scan).is_empty();
    s.add("46428-J1-双版本共存零", ok, if ok { "" } else { "双版本共存" });
    // J1-3 类别与扩展名逐件一致
    let ok = b.entries.iter().all(|e| e.kind_matches_name());
    s.add("46428-J1-类别一致", ok, if ok { "" } else { "类别漂移" });
    // J1-4 落位渲染六行齐 + 指纹稳定
    let ok = render_rt_ok(&b) && b.fingerprint() == standard_book().fingerprint();
    s.add("46428-J1-渲染与指纹", ok, if ok { "" } else { "渲染漂移" });
    // J1-5 版本一致性 + 静动态配对 + soname
    let ok = version_uniform(&b) && static_shared_pairs_ok(&b) && sonames_ok(&b);
    s.add("46428-J1-版本与配对", ok, if ok { "" } else { "版本配对漂移" });
    // J1-6 导出覆盖与目录族常量
    let (g, u) = dir_split_counts(&b);
    let ok = export_coverage_ok(&b) && dir_family_ok() && full_paths_ok(&b) && g == 3 && u == 3;
    s.add("46428-J1-导出覆盖", ok, if ok { "" } else { "覆盖漂移" });
    // 注入核 6/6
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("46428-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx46428_all_checks_green() {
        let set = run_unx46428_checks();
        assert!(set.all_passed(), "UNX-F46428 checks must be green: {}", set.red_items_text());
    }
}
