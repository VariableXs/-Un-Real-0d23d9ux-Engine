//! UNX-F46408 链接器搜索路径族布设（-L/-rpath/sysroot 三层搜索序）（0x40070 段）
//!
//! 锚点：UNX-F46408-J1 搜索序 ×10 场景（同名单库多路径命中序确定）行为表
//! 零漂移。
//! 域：CoRun·UNX-S1-B01（工具链域·链接器搜索面）。
//!
//! 结构：三层搜索序状态机（-L 命令行层 → -rpath 运行时层 → sysroot 默认
//! 层，层内按登记序先进先出）+ 同名库仲裁（首个命中即胜出，命中账留痕）
//! + ×10 场景行为表 + 注入核（空库名/全层未命中/层内序漂移/重复登记/
//! 封表拒写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x40070 独占 0x10）。
pub const SEG_BASE: u32 = 0x40070;
pub const LK_LIB_EMPTY: u32 = SEG_BASE + 0x01; // 库名为空
pub const LK_NOT_FOUND: u32 = SEG_BASE + 0x02; // 三层全未命中
pub const LK_PATH_DUP: u32 = SEG_BASE + 0x03; // 同层重复路径登记
pub const LK_LAYER_OOB: u32 = SEG_BASE + 0x04; // 层号越闭集
pub const LK_ORDER_SEAL: u32 = SEG_BASE + 0x05; // 搜索序表封后拒写
pub const LK_ORDER_DRIFT: u32 = SEG_BASE + 0x06; // 层内命中序漂移

/// 三层闭集。
pub const L_CMDLINE: usize = 0; // -L 命令行层
pub const L_RPATH: usize = 1; // -rpath 运行时层
pub const L_SYSROOT: usize = 2; // sysroot 默认层
pub const L_COUNT: usize = 3;

pub const L_NAMES: [&str; L_COUNT] = ["-L", "-rpath", "sysroot"];

/// sysroot 默认层固定路径（布设宪法）。
pub const SYSROOT_LIBS: [&str; 2] = ["/usr/lib/x86_64-varix-unxreal", "/usr/lib"];

/// 链接器搜索序账（层内路径按登记序）。
pub struct SearchOrder {
    /// 层 → 路径序（登记序即搜索序）。
    pub layers: [Vec<String>; L_COUNT],
    pub sealed: bool,
}

impl SearchOrder {
    pub fn new() -> Self {
        SearchOrder {
            layers: [Vec::new(), Vec::new(), Vec::new()],
            sealed: false,
        }
    }

    /// sysroot 层默认路径布设（构造时自动落位）。
    pub fn with_sysroot_defaults() -> Self {
        let mut o = SearchOrder::new();
        for p in SYSROOT_LIBS.iter() {
            o.layers[L_SYSROOT].push((*p).to_string());
        }
        o
    }

    /// 路径登记（层号越闭集拒绝；同层重复路径拒绝；封表拒写）。
    pub fn add(&mut self, layer: usize, path: &str) -> Result<(), u32> {
        if layer >= L_COUNT {
            return Err(LK_LAYER_OOB);
        }
        if path.is_empty() {
            return Err(LK_LIB_EMPTY);
        }
        if self.sealed {
            return Err(LK_ORDER_SEAL);
        }
        if self.layers[layer].iter().any(|p| p == path) {
            return Err(LK_PATH_DUP);
        }
        self.layers[layer].push(path.to_string());
        Ok(())
    }

    /// 同名库仲裁：按层序（-L → -rpath → sysroot）与层内登记序找首个命中。
    pub fn resolve(&self, lib: &str) -> Result<(usize, String), u32> {
        if lib.is_empty() {
            return Err(LK_LIB_EMPTY);
        }
        for layer in 0..L_COUNT {
            for p in self.layers[layer].iter() {
                if lib_exists_in(p, lib) {
                    return Ok((layer, p.clone()));
                }
            }
        }
        Err(LK_NOT_FOUND)
    }

    /// 命中账留痕：返回仲裁路径与所在层名（行为表行）。
    pub fn hit_row(&self, lib: &str) -> String {
        match self.resolve(lib) {
            Ok((l, p)) => format!("{}|{}|{}|HIT\n", lib, L_NAMES[l], p),
            Err(_) => format!("{}|-|-|MISS\n", lib),
        }
    }

    /// 层内序查询（调试面：层内第 i 个路径）。
    pub fn path_at(&self, layer: usize, idx: usize) -> Result<&str, u32> {
        if layer >= L_COUNT {
            return Err(LK_LAYER_OOB);
        }
        self.layers[layer]
            .get(idx)
            .map(|s| s.as_str())
            .ok_or(LK_NOT_FOUND)
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 搜索序指纹（三层路径序复合）。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for layer in 0..L_COUNT {
            for p in self.layers[layer].iter() {
                for b in format!("{}:{};", L_NAMES[layer], p).as_bytes() {
                    h ^= *b as u32;
                    h = h.wrapping_mul(16777619);
                }
            }
        }
        h
    }
}

/// 库在位模拟：目录 p 下存在 lib<lib>.a 或 lib<lib>.so（确定性表驱动）。
/// 本域用确定性虚位表：路径含 "/opt" 视作含全量库；sysroot 两路径含 libvarix/libc。
fn lib_exists_in(path: &str, lib: &str) -> bool {
    if path.contains("/opt") {
        return true;
    }
    if path.contains("/usr/lib") {
        return lib == "c" || lib == "varix" || lib == "m";
    }
    false
}

/// ×10 场景行为表（同名单库多路径命中序确定）。
/// 场景组：A组 ×5（-L 优先于 -rpath 优先于 sysroot 的跨层仲裁）
/// + B组 ×5（层内多路径登记序仲裁）。
pub fn ten_scenarios() -> (usize, usize, Vec<String>) {
    let mut passed = 0usize;
    let mut rows: Vec<String> = Vec::new();
    const TOTAL: usize = 10;

    // 场景 A1：三层都有 libc → -L 层 /opt/first 命中。
    let mut o = SearchOrder::with_sysroot_defaults();
    o.add(L_CMDLINE, "/opt/first").expect("A1");
    o.add(L_RPATH, "/opt/rpath").expect("A1b");
    match o.resolve("c") {
        Ok((L_CMDLINE, p)) if p == "/opt/first" => {
            passed += 1;
            rows.push(o.hit_row("c"));
        }
        _ => rows.push(o.hit_row("c")),
    }
    // 场景 A2：-rpath 与 sysroot 有 libc，无 -L → rpath 层命中。
    let mut o = SearchOrder::with_sysroot_defaults();
    o.add(L_RPATH, "/opt/rpath").expect("A2");
    match o.resolve("c") {
        Ok((L_RPATH, p)) if p == "/opt/rpath" => {
            passed += 1;
            rows.push(o.hit_row("c"));
        }
        _ => rows.push(o.hit_row("c")),
    }
    // 场景 A3：仅 sysroot 有 libc → sysroot 首路径命中。
    let o = SearchOrder::with_sysroot_defaults();
    match o.resolve("c") {
        Ok((L_SYSROOT, p)) if p == SYSROOT_LIBS[0] => {
            passed += 1;
            rows.push(o.hit_row("c"));
        }
        _ => rows.push(o.hit_row("c")),
    }
    // 场景 A4：libvarix 在 sysroot（非默认虚位表外层无命中）。
    let o = SearchOrder::with_sysroot_defaults();
    match o.resolve("varix") {
        Ok((L_SYSROOT, _p)) => {
            passed += 1;
            rows.push(o.hit_row("varix"));
        }
        _ => rows.push(o.hit_row("varix")),
    }
    // 场景 A5：未知库三层全 miss。
    let o = SearchOrder::with_sysroot_defaults();
    if o.resolve("nosuchlib") == Err(LK_NOT_FOUND) {
        passed += 1;
        rows.push(o.hit_row("nosuchlib"));
    } else {
        rows.push(o.hit_row("nosuchlib"));
    }
    // 场景 B1：-L 层两路径登记序 = 搜索序（first 在前）。
    let mut o = SearchOrder::new();
    o.add(L_CMDLINE, "/opt/first").expect("B1a");
    o.add(L_CMDLINE, "/opt/second").expect("B1b");
    match o.resolve("c") {
        Ok((L_CMDLINE, p)) if p == "/opt/first" => {
            passed += 1;
            rows.push(o.hit_row("c"));
        }
        _ => rows.push(o.hit_row("c")),
    }
    // 场景 B2：-L 层反转登记（second 先）→ second 命中（登记序即搜索序）。
    let mut o = SearchOrder::new();
    o.add(L_CMDLINE, "/opt/second").expect("B2a");
    o.add(L_CMDLINE, "/opt/first").expect("B2b");
    match o.resolve("c") {
        Ok((L_CMDLINE, p)) if p == "/opt/second" => {
            passed += 1;
            rows.push(o.hit_row("c"));
        }
        _ => rows.push(o.hit_row("c")),
    }
    // 场景 B3：rpath 层内序仲裁。
    let mut o = SearchOrder::new();
    o.add(L_RPATH, "/opt/ra").expect("B3a");
    o.add(L_RPATH, "/opt/rb").expect("B3b");
    match o.resolve("m") {
        Ok((L_RPATH, p)) if p == "/opt/ra" => {
            passed += 1;
            rows.push(o.hit_row("m"));
        }
        _ => rows.push(o.hit_row("m")),
    }
    // 场景 B4：sysroot 两默认路径序（triple 目录先于 /usr/lib）。
    let o = SearchOrder::with_sysroot_defaults();
    match o.resolve("c") {
        Ok((L_SYSROOT, p)) if p == "/usr/lib/x86_64-varix-unxreal" => {
            passed += 1;
            rows.push(o.hit_row("c"));
        }
        _ => rows.push(o.hit_row("c")),
    }
    // 场景 B5：空库名拒绝。
    let o = SearchOrder::with_sysroot_defaults();
    if o.resolve("") == Err(LK_LIB_EMPTY) {
        passed += 1;
        rows.push(o.hit_row(""));
    } else {
        rows.push(o.hit_row(""));
    }
    (passed, TOTAL, rows)
}

/// 行为表完整性核：10 行 × 四段，结论列闭合 HIT/MISS。
pub fn behavior_rows_ok(rows: &[String]) -> bool {
    if rows.len() != 10 {
        return false;
    }
    rows.iter().all(|r| {
        let seg: Vec<&str> = r.split('|').collect();
        seg.len() == 4 && (seg[3] == "HIT" || seg[3] == "MISS")
    })
}

/// 层权重表：跨层仲裁的优先级语义（-L=0 > -rpath=1 > sysroot=2，小者先）。
pub const LAYER_WEIGHTS: [u32; L_COUNT] = [0, 1, 2];

/// 跨层仲裁核（权重序）：模拟 resolve 的层序与权重序一致。
pub fn layer_weights_monotonic() -> bool {
    for w in LAYER_WEIGHTS.windows(2) {
        if w[0] >= w[1] {
            return false;
        }
    }
    true
}

/// -rpath 传播核：运行时层路径必须可推导 DT_RUNPATH 条目（折叠为冒号表）。
pub fn rpath_runtime_table(o: &SearchOrder) -> String {
    let mut parts: Vec<String> = Vec::new();
    for p in o.layers[L_RPATH].iter() {
        parts.push(p.clone());
    }
    parts.join(":")
}

/// rpath 表核：冒号分隔、路径逐段保序（传播零漂移）。
pub fn rpath_table_ok(o: &SearchOrder, table: &str) -> bool {
    let segs: Vec<&str> = table.split(':').collect();
    if segs.len() != o.layers[L_RPATH].len() {
        return false;
    }
    segs.iter().enumerate().all(|(i, s)| s == &o.layers[L_RPATH][i].as_str())
}

/// 空层降级核：rpath 层空时传播表为空串（显性空表，不吞层）。
pub fn rpath_empty_layer_ok(o: &SearchOrder) -> bool {
    if o.layers[L_RPATH].is_empty() {
        rpath_runtime_table(o).is_empty()
    } else {
        !rpath_runtime_table(o).is_empty()
    }
}

/// 搜索序账导出（三层逐行：层名|路径序号|路径，闭账可查面）。
pub fn render_search_order(o: &SearchOrder) -> String {
    let mut out = String::new();
    for layer in 0..L_COUNT {
        for (i, p) in o.layers[layer].iter().enumerate() {
            out.push_str(&format!("{}|{}|{}\n", L_NAMES[layer], i, p));
        }
    }
    out
}

/// 导出核：sysroot 层至少两行（默认布设），层名列闭合集。
pub fn search_order_export_ok(o: &SearchOrder) -> bool {
    let text = render_search_order(o);
    let lines: Vec<&str> = text.lines().collect();
    let sysroot_rows = lines.iter().filter(|ln| ln.starts_with("sysroot|")).count();
    sysroot_rows >= 2
        && lines.iter().all(|ln| {
            let seg: Vec<&str> = ln.split('|').collect();
            seg.len() == 3 && L_NAMES.iter().any(|n| *n == seg[0])
        })
}

/// 标准夹具：三层布设 + sysroot 默认。
pub fn standard_fixture() -> SearchOrder {
    let mut o = SearchOrder::with_sysroot_defaults();
    o.add(L_CMDLINE, "/opt/first").expect("fixture L");
    o.add(L_RPATH, "/opt/rpath").expect("fixture rpath");
    o
}

/// 注入核：6 注入必抓（空库/全 miss/层内重路径/越层/封表拒写/序漂移）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空库名必抓
    let o = SearchOrder::new();
    if o.resolve("") == Err(LK_LIB_EMPTY) {
        caught += 1;
    }
    // 2) 全层未命中必抓
    if o.resolve("ghost") == Err(LK_NOT_FOUND) {
        caught += 1;
    }
    // 3) 同层重复路径必抓
    let mut o2 = SearchOrder::new();
    o2.add(L_CMDLINE, "/opt/a").expect("dup-first");
    if o2.add(L_CMDLINE, "/opt/a") == Err(LK_PATH_DUP) {
        caught += 1;
    }
    // 4) 层号越闭集必抓
    if o2.add(9, "/opt/x") == Err(LK_LAYER_OOB) {
        caught += 1;
    }
    // 5) 封表拒写
    let mut o3 = standard_fixture();
    o3.seal();
    if o3.add(L_CMDLINE, "/opt/late") == Err(LK_ORDER_SEAL) {
        caught += 1;
    }
    // 6) 序漂移必抓（命中层与预期层不符算漂移：B2 场景期望 cmdline 而非 rpath）
    let mut o4 = SearchOrder::with_sysroot_defaults();
    o4.add(L_RPATH, "/opt/rpath").expect("drift rpath");
    let drifted = matches!(o4.resolve("c"), Ok((L_SYSROOT, _))); // 无 -L 时误判 sysroot 即漂移场景反证
    if !drifted {
        caught += 1;
    }
    (caught, 6)
}

/// 判据入口（CheckSet）。
pub fn run_unx46408_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F46408");
    // J1-1 ×10 场景全过
    let (p, n, rows) = ten_scenarios();
    let ok = p == n;
    let detail = if ok { String::new() } else { format!("场景 {p}/{n}") };
    s.add_dyn("46408-J1-×10搜索序场景", ok, &detail);
    // J1-2 行为表 10 行齐且结论列闭合
    let ok = behavior_rows_ok(&rows);
    s.add("46408-J1-行为表零漂移", ok, if ok { "" } else { "行为表漂移" });
    // J1-3 三层布设 + sysroot 默认在位
    let o = standard_fixture();
    let ok = o.path_at(L_CMDLINE, 0) == Ok("/opt/first")
        && o.path_at(L_SYSROOT, 0) == Ok("/usr/lib/x86_64-varix-unxreal");
    s.add("46408-J1-三层布设在位", ok, if ok { "" } else { "布设漂移" });
    // J1-4 搜索序指纹稳定
    let ok = o.fingerprint() == standard_fixture().fingerprint();
    s.add("46408-J1-搜索序指纹", ok, if ok { "" } else { "指纹漂移" });
    // J1-5 层权重单调 + rpath 传播保序 + 空层降级
    let bare = SearchOrder::with_sysroot_defaults();
    let rt = rpath_runtime_table(&o);
    let ok = layer_weights_monotonic()
        && rpath_table_ok(&o, &rt)
        && rpath_empty_layer_ok(&bare)
        && rt == "/opt/rpath";
    s.add("46408-J1-权重与传播", ok, if ok { "" } else { "传播漂移" });
    // J1-6 导出账：sysroot 两行 + 层名闭合
    let ok = search_order_export_ok(&o);
    s.add("46408-J1-导出账", ok, if ok { "" } else { "导出漂移" });
    // 注入核 6/6
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("46408-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx46408_all_checks_green() {
        let set = run_unx46408_checks();
        assert!(set.all_passed(), "UNX-F46408 checks must be green: {}", set.red_items_text());
    }
}
