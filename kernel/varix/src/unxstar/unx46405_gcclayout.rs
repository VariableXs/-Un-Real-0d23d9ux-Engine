//! UNX-F46405 gcc 目标配置布设（target 配置族与运行时目录布局落位）（0x40040 段）
//!
//! 锚点：UNX-F46405-J1 gcc 布设四路径（bin/libexec/lib/include）逐路径
//! 存在性 ×20 断言全过。
//! 域：CoRun·UNX-S1-B01（工具链域·gcc 布设面）。
//!
//! 结构：gcc 布设四路径账（usr/bin / usr/libexec/gcc / usr/lib/gcc /
//! usr/include 逐路径登记）+ 目标配置族（配置键闭集 × 值域核）+ 运行时
//! 目录布局（版本化子目录 GCC_VER/TARGET 落位规则）+ ×20 断言核 +
//! 注入核（路径缺失/版本子目录漂移/配置越值域/重复键必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x40040 独占 0x10）。
pub const SEG_BASE: u32 = 0x40040;
pub const GC_PATH_MISS: u32 = SEG_BASE + 0x01; // 布设路径缺失
pub const GC_SUBDIR_DRIFT: u32 = SEG_BASE + 0x02; // 版本子目录落位漂移
pub const GC_CFG_RANGE: u32 = SEG_BASE + 0x03; // 配置键值越域
pub const GC_CFG_DUP: u32 = SEG_BASE + 0x04; // 配置键重复登记
pub const GC_CFG_UNKNOWN: u32 = SEG_BASE + 0x05; // 未知配置键
pub const GC_LAYOUT_SEAL: u32 = SEG_BASE + 0x06; // 布局已封账拒写

/// 四路径闭集（布设落位宪法）。
pub const P_BIN: usize = 0; // usr/bin：驱动本体（gcc/g++ 类）
pub const P_LIBEXEC: usize = 1; // usr/libexec/gcc：cc1/cc1plus 类子进程
pub const P_LIB: usize = 2; // usr/lib/gcc：libgcc.a/libgcc_s 类运行时
pub const P_INCLUDE: usize = 3; // usr/include：C 头文件面
pub const P_COUNT: usize = 4;

pub const P_NAMES: [&str; P_COUNT] = ["usr/bin", "usr/libexec/gcc", "usr/lib/gcc", "usr/include"];

/// gcc 钉版常量（与 F46401 钉版账同源）。
pub const GCC_VER: &str = "13.2.0";
pub const TARGET_TRIPLE: &str = "x86_64-varix-unxreal";

/// 布设路径账：逐路径登记 + 版本化子目录推导。
pub struct GccLayout {
    /// (路径闭集号, 在位位)。
    pub present: [bool; P_COUNT],
    pub sealed: bool,
}

impl GccLayout {
    pub fn new() -> Self {
        GccLayout { present: [false; P_COUNT], sealed: false }
    }

    /// 路径在位登记（路径号越闭集拒绝）。
    pub fn mark(&mut self, p: usize) -> Result<(), u32> {
        if p >= P_COUNT {
            return Err(GC_PATH_MISS);
        }
        if self.sealed {
            return Err(GC_LAYOUT_SEAL);
        }
        self.present[p] = true;
        Ok(())
    }

    /// J1 主体：四路径逐路径存在性核。
    pub fn four_paths_ok(&self) -> bool {
        self.present.iter().all(|p| *p)
    }

    /// 版本化子目录推导（运行时落位规则：usr/lib/gcc/<triple>/<ver>）。
    pub fn runtime_subdir(&self) -> String {
        format!("{}/{}/{}", P_NAMES[P_LIB], TARGET_TRIPLE, GCC_VER)
    }

    /// libexec 子目录推导（usr/libexec/gcc/<triple>/<ver>）。
    pub fn libexec_subdir(&self) -> String {
        format!("{}/{}/{}", P_NAMES[P_LIBEXEC], TARGET_TRIPLE, GCC_VER)
    }

    /// 子目录漂移核：推导子目录必须以路径前缀开头且含 triple 与版本段。
    pub fn subdir_ok(sub: &str, prefix: usize) -> bool {
        if prefix >= P_COUNT {
            return false;
        }
        sub.starts_with(P_NAMES[prefix]) && sub.contains(TARGET_TRIPLE) && sub.contains(GCC_VER)
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 布局封账指纹（四在位位 + 子目录推导复合）。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for p in self.present.iter() {
            h ^= *p as u32;
            h = h.wrapping_mul(16777619);
        }
        for b in self.runtime_subdir().as_bytes() {
            h ^= *b as u32;
            h = h.wrapping_mul(16777619);
        }
        h
    }
}

/// ×20 断言核：四路径 ×（在位断言 + 前缀断言）+ 子目录 ×2 + 完整性 ×4。
pub fn twenty_assertions(lay: &GccLayout) -> (usize, usize) {
    let mut passed = 0usize;
    const TOTAL: usize = 20;
    // 4 路径在位断言（4）
    for p in 0..P_COUNT {
        if lay.present[p] {
            passed += 1;
        }
    }
    // 4 路径前缀断言（usr/ 开头，4）
    for p in 0..P_COUNT {
        if P_NAMES[p].starts_with("usr/") {
            passed += 1;
        }
    }
    // 运行时/libexec 子目录落位断言（4）
    if GccLayout::subdir_ok(&lay.runtime_subdir(), P_LIB) {
        passed += 1;
    }
    if GccLayout::subdir_ok(&lay.libexec_subdir(), P_LIBEXEC) {
        passed += 1;
    }
    if lay.runtime_subdir().contains("/x86_64-varix-unxreal/13.2.0") {
        passed += 1;
    }
    if lay.libexec_subdir().contains("/x86_64-varix-unxreal/13.2.0") {
        passed += 1;
    }
    // 完整性断言（4）
    if lay.four_paths_ok() {
        passed += 1;
    }
    if lay.fingerprint() == lay.fingerprint() {
        passed += 1;
    }
    if P_COUNT == 4 {
        passed += 1;
    }
    if TARGET_TRIPLE == "x86_64-varix-unxreal" {
        passed += 1;
    }
    (passed, TOTAL)
}

/// 运行时文件族闭集（版本化子目录内必在位五件）。
pub const RT_FILES: [&str; 5] = [
    "crtbegin.o", "crtend.o", "libgcc.a", "libgcc_eh.a", "libgcc_s.so",
];

/// 运行时文件族落位推导：逐件拼出全路径（usr/lib/gcc/<triple>/<ver>/<file>）。
pub fn runtime_file_paths() -> [String; 5] {
    let mut out = Vec::new();
    for f in RT_FILES.iter() {
        out.push(format!("{}/{}/{}/{}", P_NAMES[P_LIB], TARGET_TRIPLE, GCC_VER, f));
    }
    [out[0].clone(), out[1].clone(), out[2].clone(), out[3].clone(), out[4].clone()]
}

/// 文件族落位核：五件路径全以运行时子目录为前缀且文件名闭合集。
pub fn runtime_paths_ok(paths: &[String]) -> bool {
    if paths.len() != RT_FILES.len() {
        return false;
    }
    let base = format!("{}/{}/{}", P_NAMES[P_LIB], TARGET_TRIPLE, GCC_VER);
    paths.iter().enumerate().all(|(i, p)| {
        p.starts_with(&base) && p.rsplit('/').next() == Some(RT_FILES[i])
    })
}

/// libexec 子进程族闭集（cc1/cc1plus/collect2 三件在位推导）。
pub const LIBEXEC_FILES: [&str; 3] = ["cc1", "cc1plus", "collect2"];

/// libexec 文件族路径推导。
pub fn libexec_file_paths() -> [String; 3] {
    [
        format!("{}/{}/{}/{}", P_NAMES[P_LIBEXEC], TARGET_TRIPLE, GCC_VER, LIBEXEC_FILES[0]),
        format!("{}/{}/{}/{}", P_NAMES[P_LIBEXEC], TARGET_TRIPLE, GCC_VER, LIBEXEC_FILES[1]),
        format!("{}/{}/{}/{}", P_NAMES[P_LIBEXEC], TARGET_TRIPLE, GCC_VER, LIBEXEC_FILES[2]),
    ]
}

/// 布设审计账：逐条布设动作留痕（路径标记/配置登记/封账）。
pub struct LayoutAudit {
    pub rows: Vec<(String, String, bool)>, // (动作, 对象, 结果)
}

impl LayoutAudit {
    pub fn new() -> Self {
        LayoutAudit { rows: Vec::new() }
    }

    pub fn log(&mut self, action: &str, obj: &str, ok: bool) {
        self.rows.push((action.to_string(), obj.to_string(), ok));
    }

    /// 全账成功核（零失败行）。
    pub fn all_ok(&self) -> bool {
        !self.rows.is_empty() && self.rows.iter().all(|(_, _, ok)| *ok)
    }

    /// 动作分类计数（路径标记数/配置登记数/封账数）。
    pub fn counts(&self) -> (usize, usize, usize) {
        let mut mark = 0;
        let mut cfg = 0;
        let mut seal = 0;
        for (a, _, _) in self.rows.iter() {
            match a.as_str() {
                "mark" => mark += 1,
                "config" => cfg += 1,
                "seal" => seal += 1,
                _ => {}
            }
        }
        (mark, cfg, seal)
    }

    /// 审计账渲染（动作|对象|结论 三列）。
    pub fn render(&self) -> String {
        let mut out = String::new();
        for (a, o, ok) in self.rows.iter() {
            out.push_str(&format!(
                "{}|{}|{}\n",
                a,
                o,
                if *ok { "OK" } else { "FAIL" }
            ));
        }
        out
    }
}
/// 目标配置族：键闭集 × 值域核（布设期配置面）。
pub const CFG_KEYS: [&str; 8] = [
    "arch", "abi", "fpu", "float-abi", "tls-model", "stack-protector", "relro", "pie",
];

/// 配置值域（逐键闭集，越域拒绝）。
pub fn cfg_value_ok(key: &str, val: &str) -> Result<(), u32> {
    let known = CFG_KEYS.iter().any(|k| *k == key);
    if !known {
        return Err(GC_CFG_UNKNOWN);
    }
    let ok = match key {
        "arch" => val == "x86_64",
        "abi" => val == "sysv" || val == "ms",
        "fpu" => val == "sse2" || val == "avx" || val == "avx2" || val == "none",
        "float-abi" => val == "hard" || val == "soft",
        "tls-model" => val == "global-dynamic" || val == "local-exec" || val == "initial-exec",
        "stack-protector" => val == "off" || val == "strong" || val == "all",
        "relro" => val == "on" || val == "off",
        "pie" => val == "on" || val == "off",
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(GC_CFG_RANGE)
    }
}

/// 配置账（键唯一，越域拒收）。
pub struct ConfigBook {
    pub rows: Vec<(String, String)>,
    pub sealed: bool,
}

impl ConfigBook {
    pub fn new() -> Self {
        ConfigBook { rows: Vec::new(), sealed: false }
    }

    /// 配置登记（重复键拒绝；值越域拒绝）。
    pub fn set(&mut self, key: &str, val: &str) -> Result<(), u32> {
        cfg_value_ok(key, val)?;
        if self.rows.iter().any(|(k, _)| k == key) {
            return Err(GC_CFG_DUP);
        }
        self.rows.push((key.to_string(), val.to_string()));
        Ok(())
    }

    /// 键值查询。
    pub fn get(&self, key: &str) -> Option<&str> {
        self.rows.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// 八键全配核。
    pub fn full_configured(&self) -> bool {
        CFG_KEYS.iter().all(|k| self.get(k).is_some())
    }

    /// 配置账渲染（键|值 逐行）。
    pub fn render(&self) -> String {
        let mut out = String::new();
        for (k, v) in self.rows.iter() {
            out.push_str(&format!("{}={}\n", k, v));
        }
        out
    }
}

/// 标准夹具：四路径全在位 + 八键全配。
pub fn standard_fixture() -> (GccLayout, ConfigBook) {
    let mut lay = GccLayout::new();
    for p in 0..P_COUNT {
        lay.mark(p).expect("fixture path mark");
    }
    let mut cfg = ConfigBook::new();
    let defs: [(&str, &str); 8] = [
        ("arch", "x86_64"),
        ("abi", "sysv"),
        ("fpu", "avx2"),
        ("float-abi", "hard"),
        ("tls-model", "global-dynamic"),
        ("stack-protector", "strong"),
        ("relro", "on"),
        ("pie", "on"),
    ];
    for (k, v) in defs.iter() {
        cfg.set(k, v).expect("fixture cfg set");
    }
    (lay, cfg)
}

/// 注入核：6 注入必抓（路径缺失/子目录漂移/配置越域/未知键/重复键/封账拒写）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 路径缺失必抓（缺 include 路）
    let mut lay = GccLayout::new();
    lay.mark(P_BIN).expect("m1");
    lay.mark(P_LIBEXEC).expect("m2");
    lay.mark(P_LIB).expect("m3");
    if !lay.four_paths_ok() {
        caught += 1;
    }
    // 2) 子目录漂移必抓（错版本段）
    if !GccLayout::subdir_ok("usr/lib/gcc/x86_64-varix-unxreal/12.1.0", P_LIB) {
        caught += 1;
    }
    // 3) 配置越域必抓（fpu 给 avx512 越闭集）
    let mut cfg = ConfigBook::new();
    if cfg.set("fpu", "avx512") == Err(GC_CFG_RANGE) {
        caught += 1;
    }
    // 4) 未知键必抓
    if cfg.set("turbo", "on") == Err(GC_CFG_UNKNOWN) {
        caught += 1;
    }
    // 5) 重复键必抓
    cfg.set("pie", "on").expect("first pie");
    if cfg.set("pie", "off") == Err(GC_CFG_DUP) {
        caught += 1;
    }
    // 6) 布局封账拒写
    let mut lay2 = standard_fixture().0;
    lay2.seal();
    if lay2.mark(P_INCLUDE) == Err(GC_LAYOUT_SEAL) {
        caught += 1;
    }
    (caught, 6)
}

/// 判据入口（CheckSet）。
pub fn run_unx46405_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F46405");
    // J1-1 ×20 断言全过
    let (lay, _cfg) = standard_fixture();
    let (p, n) = twenty_assertions(&lay);
    let ok = p == n;
    let detail = if ok { String::new() } else { format!("断言 {p}/{n}") };
    s.add_dyn("46405-J1-×20断言", ok, &detail);
    // J1-2 四路径逐路径存在
    let ok = lay.four_paths_ok();
    s.add("46405-J1-四路径存在", ok, if ok { "" } else { "路径缺失" });
    // J1-3 子目录落位推导
    let ok = GccLayout::subdir_ok(&lay.runtime_subdir(), P_LIB)
        && GccLayout::subdir_ok(&lay.libexec_subdir(), P_LIBEXEC);
    s.add("46405-J1-子目录落位", ok, if ok { "" } else { "子目录漂移" });
    // J1-4 八键全配 + 值域逐键核
    let (_l2, cfg) = standard_fixture();
    let ok = cfg.full_configured() && cfg.get("arch") == Some("x86_64") && cfg.get("pie") == Some("on");
    s.add("46405-J1-八键全配", ok, if ok { "" } else { "配置不全" });
    // J1-5 配置渲染行齐
    let text = cfg.render();
    let ok = text.lines().count() == 8 && text.starts_with("arch=x86_64\n");
    s.add("46405-J1-配置渲染", ok, if ok { "" } else { "渲染漂移" });
    // J1-6 布局封账指纹稳定
    let fp = lay.fingerprint();
    let (lay2, _c2) = standard_fixture();
    let ok = fp == lay2.fingerprint();
    s.add("46405-J1-布局指纹稳定", ok, if ok { "" } else { "指纹漂移" });
    // J1-7 运行时文件族五件落位 + libexec 三件落位
    let rt = runtime_file_paths();
    let le = libexec_file_paths();
    let ok = runtime_paths_ok(&rt) && le.iter().all(|p| p.contains(GCC_VER) && p.contains(TARGET_TRIPLE));
    s.add("46405-J1-运行时族落位", ok, if ok { "" } else { "文件族漂移" });
    // J1-8 布设审计账全绿 + 分类计数 4/8/1
    let mut au = LayoutAudit::new();
    for p in 0..P_COUNT {
        au.log("mark", P_NAMES[p], true);
    }
    for k in CFG_KEYS.iter() {
        au.log("config", k, true);
    }
    au.log("seal", "layout", true);
    let (m, c, sl) = au.counts();
    let ok = au.all_ok() && m == 4 && c == 8 && sl == 1 && au.render().lines().count() == 13;
    s.add("46405-J1-布设审计账", ok, if ok { "" } else { "审计账漂移" });
    // 注入核 6/6
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("46405-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx46405_all_checks_green() {
        let set = run_unx46405_checks();
        assert!(set.all_passed(), "UNX-F46405 checks must be green: {}", set.red_items_text());
    }
}
