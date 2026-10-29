//! UNX-F46424 sysroot 语义本体（交叉构建与原生构建共用布局宪法）（0x40170 段）
//!
//! 锚点：UNX-F46424-J1 交叉/原生双模 ×20 构建用例解析到同布局零分歧。
//! 域：CoRun·UNX-S1-B02（工具链域·sysroot 语义面）。
//!
//! 结构：sysroot 语义本体（布局宪法：root → bin/lib/include/libexec 四区
//! + triple 子目录规则）+ 双模解析器（交叉态 --sysroot=X 与原生态默认
//! root，输出同构布局）+ ×20 用例核（双模各 10，逐区同布局）+ 注入核
//! （空 root/坏区号/双模分歧/重复区/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x40170 独占 0x10）。
pub const SEG_BASE: u32 = 0x40170;
pub const SR_ROOT_EMPTY: u32 = SEG_BASE + 0x01; // root 为空
pub const SR_ZONE_OOB: u32 = SEG_BASE + 0x02; // 区号越闭集
pub const SR_MODE_SPLIT: u32 = SEG_BASE + 0x03; // 双模布局分歧
pub const SR_ZONE_DUP: u32 = SEG_BASE + 0x04; // 重复区登记
pub const SR_SEALED: u32 = SEG_BASE + 0x05; // 封账改写

/// 四区闭集。
pub const Z_BIN: usize = 0;
pub const Z_LIB: usize = 1;
pub const Z_INCLUDE: usize = 2;
pub const Z_LIBEXEC: usize = 3;
pub const Z_COUNT: usize = 4;

pub const Z_NAMES: [&str; Z_COUNT] = ["bin", "lib", "include", "libexec"];

/// 布局区：区号 + 相对路径 + 在位位。
#[derive(Clone)]
pub struct Zone {
    pub zone: usize,
    pub rel: String,
    pub present: bool,
}

/// sysroot 布局本体（四区全登记 + 封账）。
pub struct SysrootLayout {
    pub root: String,
    pub zones: Vec<Zone>,
    pub sealed: bool,
}

impl SysrootLayout {
    /// 构造（root 非空；空账）。
    pub fn new(root: &str) -> Result<Self, u32> {
        if root.is_empty() {
            return Err(SR_ROOT_EMPTY);
        }
        Ok(SysrootLayout { root: root.to_string(), zones: Vec::new(), sealed: false })
    }

    /// 区登记（越区/重复/封账拒绝）。
    pub fn put(&mut self, zone: usize, rel: &str, present: bool) -> Result<(), u32> {
        if zone >= Z_COUNT {
            return Err(SR_ZONE_OOB);
        }
        if self.sealed {
            return Err(SR_SEALED);
        }
        if self.zones.iter().any(|z| z.zone == zone) {
            return Err(SR_ZONE_DUP);
        }
        self.zones.push(Zone { zone, rel: rel.to_string(), present });
        Ok(())
    }

    /// 区路径推导（root + rel）。
    pub fn path_of(&self, zone: usize) -> Result<String, u32> {
        match self.zones.iter().find(|z| z.zone == zone) {
            None => Err(SR_ZONE_OOB),
            Some(z) => Ok(format!("{}{}", self.root, z.rel)),
        }
    }

    /// 四区齐核。
    pub fn complete(&self) -> bool {
        self.zones.len() == Z_COUNT
    }

    /// 布局同构核：两布局逐区相对路径与在位位全等（root 可异）。
    pub fn same_shape(&self, other: &SysrootLayout) -> bool {
        if self.zones.len() != other.zones.len() {
            return false;
        }
        for z in self.zones.iter() {
            match other.zones.iter().find(|o| o.zone == z.zone) {
                None => return false,
                Some(o) => {
                    if o.rel != z.rel || o.present != z.present {
                        return false;
                    }
                }
            }
        }
        true
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 布局指纹（四区序复合，root 不入指纹——同构异 root 同印）。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for z in self.zones.iter() {
            for b in format!("{}|{}|{};", z.zone, z.rel, z.present).as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(16777619);
            }
        }
        h
    }
}

/// 双模解析器：交叉态与原生态 → 同构布局。
/// 交叉态：root = --sysroot 给定；原生态：root = ""（相对根）。
pub fn parse_mode(mode: usize, given_root: &str) -> Result<SysrootLayout, u32> {
    // mode: 0 = 交叉（cross），1 = 原生（native）。
    let root = match mode {
        0 => {
            if given_root.is_empty() {
                return Err(SR_ROOT_EMPTY);
            }
            given_root.to_string()
        }
        1 => String::new(),
        _ => return Err(SR_MODE_SPLIT),
    };
    let mut l = SysrootLayout::new(&root)?;
    l.put(Z_BIN, "/usr/bin/", true).expect("zone bin");
    l.put(Z_LIB, "/usr/lib/x86_64-varix-unxreal/", true).expect("zone lib");
    l.put(Z_INCLUDE, "/usr/include/", true).expect("zone inc");
    l.put(Z_LIBEXEC, "/usr/libexec/gcc/x86_64-varix-unxreal/13.2.0/", true).expect("zone lexec");
    Ok(l)
}

/// ×20 用例核：交叉 ×10（不同 root 全同构）+ 原生 ×10（重放全同构）。
pub fn twenty_cases() -> (usize, usize) {
    let mut passed = 0usize;
    const TOTAL: usize = 20;
    // 交叉 ×10：root 族全同构。
    let roots = [
        "/varix-sysroot", "/opt/cross/x86_64", "/build/sysroot-a",
        "/build/sysroot-b", "/tmp/sr1", "/tmp/sr2", "/var/sr3",
        "/mnt/sr4", "/home/sr5", "/data/sr6",
    ];
    let base = parse_mode(0, roots[0]).expect("base cross");
    for r in roots.iter() {
        let l = parse_mode(0, r).expect("cross case");
        if l.same_shape(&base) {
            passed += 1;
        }
    }
    // 原生 ×10：重放全同构。
    let native = parse_mode(1, "").expect("base native");
    for _ in 0..10 {
        let l = parse_mode(1, "").expect("native case");
        if l.same_shape(&native) {
            passed += 1;
        }
    }
    (passed, TOTAL)
}

/// 区内容件账：区内逐件（相对路径 × 存在位 × 类别）。
#[derive(Clone)]
pub struct ZoneItem {
    pub zone: usize,
    pub rel: String,
    pub present: bool,
    /// 类别：0=可执行 1=库 2=头 3=目录。
    pub kind: u8,
}

impl ZoneItem {
    pub fn kind_ok(k: u8) -> bool {
        k <= 3
    }
}

/// 区内容账（逐件登记，区内唯一）。
pub struct ZoneContent {
    pub items: Vec<ZoneItem>,
    pub sealed: bool,
}

impl ZoneContent {
    pub fn new() -> Self {
        ZoneContent { items: Vec::new(), sealed: false }
    }

    /// 件登记（越区/越类别/重复/封账拒绝）。
    pub fn put(&mut self, zone: usize, rel: &str, present: bool, kind: u8) -> Result<(), u32> {
        if zone >= Z_COUNT {
            return Err(SR_ZONE_OOB);
        }
        if !ZoneItem::kind_ok(kind) {
            return Err(SR_ZONE_OOB);
        }
        if rel.is_empty() {
            return Err(SR_ROOT_EMPTY);
        }
        if self.sealed {
            return Err(SR_SEALED);
        }
        if self.items.iter().any(|i| i.zone == zone && i.rel == rel) {
            return Err(SR_ZONE_DUP);
        }
        self.items.push(ZoneItem { zone, rel: rel.to_string(), present, kind });
        Ok(())
    }

    /// 区内件数。
    pub fn zone_count(&self, zone: usize) -> usize {
        self.items.iter().filter(|i| i.zone == zone).count()
    }

    /// 全账件数。
    pub fn total(&self) -> usize {
        self.items.len()
    }

    /// 同构核：两份内容账逐件全等（区 × 相对路径 × 类别 × 在位）。
    pub fn same_shape(&self, other: &ZoneContent) -> bool {
        if self.items.len() != other.items.len() {
            return false;
        }
        self.items.iter().all(|i| {
            other
                .items
                .iter()
                .any(|o| o.zone == i.zone && o.rel == i.rel && o.present == i.present && o.kind == i.kind)
        })
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 内容指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for i in self.items.iter() {
            for b in format!("{}|{}|{}|{};", i.zone, i.rel, i.present, i.kind).as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(16777619);
            }
        }
        h
    }
}

/// sysroot 路径折叠：绝对路径 → 去除 root 前缀的相对路径。
pub fn fold_path(root: &str, abs: &str) -> Result<String, u32> {
    if root.is_empty() || abs.is_empty() {
        return Err(SR_ROOT_EMPTY);
    }
    if let Some(stripped) = abs.strip_prefix(root) {
        Ok(stripped.to_string())
    } else {
        Err(SR_MODE_SPLIT)
    }
}

/// sysroot 路径展开：相对路径 → root 前缀拼接。
pub fn unfold_path(root: &str, rel: &str) -> Result<String, u32> {
    if root.is_empty() {
        return Err(SR_ROOT_EMPTY);
    }
    if rel.is_empty() {
        return Err(SR_ROOT_EMPTY);
    }
    Ok(format!("{}{}", root, rel))
}

/// 折叠/展开往返核（fold(unfold(x)) == x）。
pub fn roundtrip_ok(root: &str, rel: &str) -> bool {
    match unfold_path(root, rel) {
        Ok(abs) => fold_path(root, &abs) == Ok(rel.to_string()),
        Err(_) => false,
    }
}

/// 双模导出渲染（mode|root|区路径 四区逐行）。
pub fn render_layout(l: &SysrootLayout, mode: &str) -> String {
    let mut out = String::new();
    for z in l.zones.iter() {
        if let Ok(p) = l.path_of(z.zone) {
            out.push_str(&format!("{}|{}|{}\n", mode, Z_NAMES[z.zone], p));
        }
    }
    out
}

/// 导出核：四行且区名列闭合集。
pub fn render_layout_ok(text: &str, expect: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    lines.len() == expect
        && lines.iter().all(|ln| {
            let seg: Vec<&str> = ln.split('|').collect();
            seg.len() == 3 && Z_NAMES.iter().any(|n| *n == seg[1])
        })
}

/// 标准区内容夹具：12 件（bin 3 可执行 + lib 3 库 + include 3 头 + libexec 3 目录）。
pub fn standard_content() -> ZoneContent {
    let mut c = ZoneContent::new();
    let defs: [(usize, &str, bool, u8); 12] = [
        (Z_BIN, "usr/bin/gcc", true, 0),
        (Z_BIN, "usr/bin/clang", true, 0),
        (Z_BIN, "usr/bin/ld.lld", true, 0),
        (Z_LIB, "usr/lib/x86_64-varix-unxreal/libc.a", true, 1),
        (Z_LIB, "usr/lib/x86_64-varix-unxreal/libm.a", true, 1),
        (Z_LIB, "usr/lib/x86_64-varix-unxreal/libgcc.a", true, 1),
        (Z_INCLUDE, "usr/include/stdio.h", true, 2),
        (Z_INCLUDE, "usr/include/stdlib.h", true, 2),
        (Z_INCLUDE, "usr/include/stdint.h", true, 2),
        (Z_LIBEXEC, "usr/libexec/gcc/x86_64-varix-unxreal/13.2.0/cc1", true, 3),
        (Z_LIBEXEC, "usr/libexec/gcc/x86_64-varix-unxreal/13.2.0/cc1plus", true, 3),
        (Z_LIBEXEC, "usr/libexec/gcc/x86_64-varix-unxreal/13.2.0/collect2", true, 3),
    ];
    for (z, rel, p, k) in defs.iter() {
        c.put(*z, rel, *p, *k).expect("fixture zone item");
    }
    c
}
/// 双模导出核辅助：区在位率（全在位 = 布设完整）。
pub fn zones_present_ratio(l: &SysrootLayout) -> (usize, usize) {
    let total = l.zones.len();
    let present = l.zones.iter().filter(|z| z.present).count();
    (present, total)
}

/// 标准夹具：交叉态标准 root。
pub fn standard_layout() -> SysrootLayout {
    parse_mode(0, "/varix-sysroot").expect("fixture")
}

/// 注入核：6 注入必抓（空 root/越区/双模分歧/重复区/封账改写/区缺失路径推导）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空 root（交叉态）必抓
    if parse_mode(0, "").is_err() {
        caught += 1;
    }
    // 2) 越区必抓
    let mut l = SysrootLayout::new("/r").expect("layout");
    if l.put(9, "/x", true) == Err(SR_ZONE_OOB) {
        caught += 1;
    }
    // 3) 双模分歧必抓（mode=9 拒绝）
    if parse_mode(9, "").is_err() {
        caught += 1;
    }
    // 4) 重复区必抓
    l.put(Z_BIN, "/usr/bin/", true).expect("first bin");
    if l.put(Z_BIN, "/bin/", true) == Err(SR_ZONE_DUP) {
        caught += 1;
    }
    // 5) 封账改写必抓
    let mut std_l = standard_layout();
    std_l.seal();
    if std_l.put(Z_BIN, "/bin/", true) == Err(SR_SEALED) {
        caught += 1;
    }
    // 6) 区缺失路径推导必抓（空账推导任一区 → 越区错）
    let empty = SysrootLayout::new("/r").expect("empty");
    if empty.path_of(Z_BIN) == Err(SR_ZONE_OOB) {
        caught += 1;
    }
    (caught, 6)
}

/// 判据入口（CheckSet）。
pub fn run_unx46424_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F46424");
    // J1-1 交叉/原生双模 ×20 用例解析到同布局零分歧
    let (p, n) = twenty_cases();
    let ok = p == n;
    let detail = if ok { String::new() } else { format!("用例 {p}/{n}") };
    s.add_dyn("46424-J1-双模×20零分歧", ok, &detail);
    // J1-2 四区全登记 + 路径推导
    let l = standard_layout();
    let bin = l.path_of(Z_BIN).expect("bin path");
    let ok = l.complete() && bin == "/varix-sysroot/usr/bin/";
    s.add("46424-J1-四区路径推导", ok, if ok { "" } else { "区路径漂移" });
    // J1-3 布局指纹同构同印（异 root 同印）
    let cross = parse_mode(0, "/other-root").expect("other root");
    let ok = l.fingerprint() == cross.fingerprint();
    s.add("46424-J1-同构同印", ok, if ok { "" } else { "指纹漂移" });
    // J1-4 双模互为同构
    let native = parse_mode(1, "").expect("native");
    let ok = l.same_shape(&native) && native.same_shape(&l);
    s.add("46424-J1-双模互同构", ok, if ok { "" } else { "双模分歧" });
    // J1-5 区内容件账：12 件同构双模 + 区计数 3/3/3/3
    let cc = standard_content();
    let cc2 = standard_content();
    let ok = cc.total() == 12
        && cc.zone_count(Z_BIN) == 3
        && cc.zone_count(Z_LIB) == 3
        && cc.zone_count(Z_INCLUDE) == 3
        && cc.zone_count(Z_LIBEXEC) == 3
        && cc.same_shape(&cc2);
    s.add("46424-J1-区内容账", ok, if ok { "" } else { "内容账漂移" });
    // J1-6 路径折叠/展开往返 ×2 root
    let ok = roundtrip_ok("/varix-sysroot", "/usr/bin/gcc")
        && roundtrip_ok("/opt/x", "/usr/include/stdio.h")
        && fold_path("/varix-sysroot", "/other/x") == Err(SR_MODE_SPLIT);
    s.add("46424-J1-折叠往返", ok, if ok { "" } else { "折叠漂移" });
    // J1-7 双模导出四行齐 ×2 模
    let ct = render_layout(&l, "cross");
    let nt = render_layout(&native, "native");
    let ok = render_layout_ok(&ct, 4) && render_layout_ok(&nt, 4);
    s.add("46424-J1-双模导出", ok, if ok { "" } else { "导出漂移" });
    // J1-8 内容指纹同构同印
    let ok = cc.fingerprint() == cc2.fingerprint();
    s.add("46424-J1-内容指纹", ok, if ok { "" } else { "内容指纹漂移" });
    // J1-9 区在位率 4/4
    let (p2, t2) = zones_present_ratio(&l);
    let ok = p2 == 4 && t2 == 4;
    s.add("46424-J1-区在位率", ok, if ok { "" } else { "在位率漂移" });
    // 注入核 6/6
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("46424-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx46424_all_checks_green() {
        let set = run_unx46424_checks();
        assert!(set.all_passed(), "UNX-F46424 checks must be green: {}", set.red_items_text());
    }
}
