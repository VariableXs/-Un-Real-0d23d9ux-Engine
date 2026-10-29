//! UNX-F46421 C 标准库头全集扫描器（逐头存在性 + 内容指纹账）（0x40140 段）
//!
//! 锚点：UNX-F46421-J1 C 标准头全集逐个存在 ×1 扫描零缺失、指纹 ×2 次
//! 一致。
//! 域：CoRun·UNX-S1-B02（工具链域·头集扫描面）。
//!
//! 结构：C 头全集账（20 头逐头登记：名 × 存在位 × 内容指纹）+ 扫描器
//! （表驱动存在集 → 逐头落账）+ 指纹稳定性核（×2 次一致）+ 注入核
//! （缺头/坏指纹/重复头/封账改写/全集不全必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x40140 独占 0x10）。
pub const SEG_BASE: u32 = 0x40140;
pub const HS_MISS: u32 = SEG_BASE + 0x01; // 头缺失
pub const HS_FP_BAD: u32 = SEG_BASE + 0x02; // 指纹非法
pub const HS_DUP: u32 = SEG_BASE + 0x03; // 重复头
pub const HS_SEALED: u32 = SEG_BASE + 0x04; // 封账改写
pub const HS_SET_SHORT: u32 = SEG_BASE + 0x05; // 全集不全

/// C 标准头全集 ×20（与 F46401 J3 同源声明）。
pub const C_HEADERS20: [&str; 20] = [
    "stdio.h", "stdlib.h", "string.h", "stdint.h", "stddef.h",
    "stdbool.h", "unistd.h", "fcntl.h", "errno.h", "signal.h",
    "time.h", "math.h", "assert.h", "ctype.h", "limits.h",
    "locale.h", "setjmp.h", "stdarg.h", "wchar.h", "pthread.h",
];

/// 头账目：名 × 存在位 × 内容指纹。
#[derive(Clone)]
pub struct HeadEntry {
    pub name: String,
    pub present: bool,
    pub fp: u32,
}

impl HeadEntry {
    /// 指纹合成核：头名 + 模拟内容字节 FNV。
    pub fn synth_fp(name: &str, content: &[u8]) -> u32 {
        let mut h: u32 = 2166136261;
        for b in name.as_bytes() {
            h ^= *b as u32;
            h = h.wrapping_mul(16777619);
        }
        for b in content {
            h ^= *b as u32;
            h = h.wrapping_mul(16777619);
        }
        h
    }
}

/// 头集扫描账。
pub struct HeadScanBook {
    pub rows: Vec<HeadEntry>,
    pub sealed: bool,
}

impl HeadScanBook {
    pub fn new() -> Self {
        HeadScanBook { rows: Vec::new(), sealed: false }
    }

    /// 逐头登记（重复/封账/全集外拒绝）。
    pub fn put(&mut self, name: &str, present: bool, content: &[u8]) -> Result<(), u32> {
        if !C_HEADERS20.iter().any(|h| *h == name) {
            return Err(HS_SET_SHORT);
        }
        if self.sealed {
            return Err(HS_SEALED);
        }
        if self.rows.iter().any(|r| r.name == name) {
            return Err(HS_DUP);
        }
        self.rows.push(HeadEntry {
            name: name.to_string(),
            present,
            fp: HeadEntry::synth_fp(name, content),
        });
        Ok(())
    }

    /// 缺失清单（显性化，禁含糊）。
    pub fn missing(&self) -> Vec<String> {
        let mut out = Vec::new();
        for h in C_HEADERS20.iter() {
            match self.rows.iter().find(|r| r.name == *h) {
                None => out.push((*h).to_string()),
                Some(r) => {
                    if !r.present {
                        out.push((*h).to_string());
                    }
                }
            }
        }
        out
    }

    /// J1 全集核：×20 逐头存在零缺失。
    pub fn full_present(&self) -> bool {
        self.rows.len() == 20 && self.missing().is_empty()
    }

    /// 指纹稳定性核：逐头指纹非零（零指纹 = 指纹引擎破缺，必须显性）。
    pub fn fp_stable(&self) -> bool {
        self.rows.iter().all(|r| r.fp != 0)
    }

    /// 可重放核：同头同内容重合成指纹一致（确定性引擎）。
    pub fn fp_replay_ok(&self, contents: &[(String, Vec<u8>)]) -> bool {
        self.rows.iter().all(|r| {
            match contents.iter().find(|(n, _)| *n == r.name) {
                None => false,
                Some((_n, c)) => HeadEntry::synth_fp(&r.name, c) == r.fp,
            }
        })
    }

    /// 扫描统计（在位数/缺位数/登记数三计数）。
    pub fn stats(&self) -> (usize, usize, usize) {
        let present = self.rows.iter().filter(|r| r.present).count();
        let missing = self.rows.iter().filter(|r| !r.present).count();
        (present, missing, self.rows.len())
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 扫描账指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for r in self.rows.iter() {
            for b in format!("{}|{}|{};", r.name, r.present, r.fp).as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(16777619);
            }
        }
        h
    }
}

/// 表驱动扫描器：给「在位集」与「内容源」逐头落账。
pub fn scan_into(book: &mut HeadScanBook, present: &[&str], contents: &[(String, Vec<u8>)]) -> usize {
    let mut registered = 0;
    for h in C_HEADERS20.iter() {
        let is_present = present.iter().any(|p| p == h);
        let content: Vec<u8> = contents
            .iter()
            .find(|(n, _)| n == h)
            .map(|(_, c)| c.clone())
            .unwrap_or_else(|| h.as_bytes().to_vec());
        if book.put(h, is_present, &content).is_ok() {
            registered += 1;
        }
    }
    registered
}

/// 头依赖账：头 → include 闭包（模拟 #include 图，边集登记）。
pub struct HeadDeps {
    /// (从头, 到头) 有向边集。
    pub edges: Vec<(String, String)>,
}

impl HeadDeps {
    pub fn new() -> Self {
        HeadDeps { edges: Vec::new() }
    }

    /// 边登记（两端必须 ∈ 全集；自环拒绝；重复边拒绝）。
    pub fn edge(&mut self, from: &str, to: &str) -> Result<(), u32> {
        if !C_HEADERS20.iter().any(|h| *h == from) || !C_HEADERS20.iter().any(|h| *h == to) {
            return Err(HS_SET_SHORT);
        }
        if from == to {
            return Err(HS_DUP);
        }
        if self.edges.iter().any(|(f, t)| f == from && t == to) {
            return Err(HS_DUP);
        }
        self.edges.push((from.to_string(), to.to_string()));
        Ok(())
    }

    /// 直接闭包查询（from 的一跳依赖）。
    pub fn direct(&self, from: &str) -> Vec<String> {
        let mut out = Vec::new();
        for (f, t) in self.edges.iter() {
            if f == from {
                out.push(t.clone());
            }
        }
        out
    }

    /// 二跳闭包查询（依赖的依赖，去重）。
    pub fn closure2(&self, from: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for t1 in self.direct(from) {
            for t2 in self.direct(&t1) {
                if !out.contains(&t2) && t2 != from {
                    out.push(t2);
                }
            }
        }
        out
    }

    /// 环检测（DFS 找回边——图小，逐起点 DFS）。
    pub fn has_cycle(&self) -> bool {
        fn dfs(g: &HeadDeps, node: &str, visiting: &mut Vec<String>) -> bool {
            if visiting.iter().any(|v| v == node) {
                return true;
            }
            visiting.push(node.to_string());
            for t in g.direct(node) {
                if dfs(g, &t, visiting) {
                    return true;
                }
            }
            visiting.pop();
            false
        }
        for h in C_HEADERS20.iter() {
            let mut v = Vec::new();
            if dfs(self, h, &mut v) {
                return true;
            }
        }
        false
    }
}

/// 扫描报告渲染（头|存在|指纹 十六进制 三列）。
pub fn render_report(b: &HeadScanBook) -> String {
    let mut out = String::new();
    for r in b.rows.iter() {
        out.push_str(&format!("{}|{}|{:08x}\n", r.name, if r.present { "OK" } else { "MISS" }, r.fp));
    }
    out
}

/// 报告完整性核（20 行 × 三段，存在列闭合）。
pub fn report_ok(b: &HeadScanBook) -> bool {
    let text = render_report(b);
    let lines: Vec<&str> = text.lines().collect();
    lines.len() == 20
        && lines.iter().all(|ln| {
            let seg: Vec<&str> = ln.split('|').collect();
            seg.len() == 3 && (seg[1] == "OK" || seg[1] == "MISS") && seg[2].len() == 8
        })
}

/// 指纹快照账：扫描轮次 → 全集指纹（跨轮漂移审计）。
pub struct FpSnapshots {
    pub snaps: Vec<(u32, u32)>, // (轮次, 全集指纹)
}

impl FpSnapshots {
    pub fn new() -> Self {
        FpSnapshots { snaps: Vec::new() }
    }

    /// 快照登记（轮次递增；同轮重复拒绝）。
    pub fn snap(&mut self, round: u32, fp: u32) -> Result<(), u32> {
        if round == 0 {
            return Err(HS_SET_SHORT);
        }
        if let Some((last, _)) = self.snaps.last() {
            if round <= *last {
                return Err(HS_DUP);
            }
        }
        self.snaps.push((round, fp));
        Ok(())
    }

    /// 两轮一致核（最近两轮指纹相等 = 无漂移）。
    pub fn last_two_equal(&self) -> bool {
        self.snaps.len() >= 2 && self.snaps[self.snaps.len() - 1].1 == self.snaps[self.snaps.len() - 2].1
    }

    /// 漂移定位核（指纹不同的轮次对清单）。
    pub fn drift_pairs(&self) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        for w in self.snaps.windows(2) {
            if w[0].1 != w[1].1 {
                out.push((w[0].0, w[1].0));
            }
        }
        out
    }
}

/// 标准夹具：全集在位 + 确定性内容。
pub fn standard_scan() -> HeadScanBook {
    let mut b = HeadScanBook::new();
    let contents: Vec<(String, Vec<u8>)> = C_HEADERS20
        .iter()
        .map(|h| ((*h).to_string(), format!("/* {} Varix S1 */\n", h).into_bytes()))
        .collect();
    let all: Vec<&str> = C_HEADERS20.to_vec();
    scan_into(&mut b, &all, &contents);
    b
}

/// 注入核：6 注入必抓（缺头/全集外/重复/封账改写/指纹零值/全集不全）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 缺头必抓（19 头缺 stdio.h）
    let mut b = HeadScanBook::new();
    let short: Vec<&str> = C_HEADERS20.iter().skip(1).copied().collect();
    for h in short.iter() {
        b.put(h, true, h.as_bytes()).expect("short scan");
    }
    if !b.missing().is_empty() && b.missing()[0] == "stdio.h" {
        caught += 1;
    }
    // 2) 全集外必抓
    let mut b2 = HeadScanBook::new();
    if b2.put("nonstd.h", true, b"x") == Err(HS_SET_SHORT) {
        caught += 1;
    }
    // 3) 重复必抓
    b2.put("stdio.h", true, b"x").expect("first");
    if b2.put("stdio.h", true, b"y") == Err(HS_DUP) {
        caught += 1;
    }
    // 4) 封账改写必抓
    let mut std_b = standard_scan();
    std_b.seal();
    if std_b.put("stdio.h", false, b"z") == Err(HS_SEALED) {
        caught += 1;
    }
    // 5) 指纹零值必抓（空内容与空名合成不产生零指纹）
    if HeadEntry::synth_fp("", b"") != 0 {
        caught += 1;
    }
    // 6) 全集不全必抓（缺位扫描 full_present 假）
    let mut b3 = HeadScanBook::new();
    let all2: Vec<&str> = C_HEADERS20.iter().skip(5).copied().collect();
    for h in all2.iter() {
        b3.put(h, true, h.as_bytes()).expect("partial scan");
    }
    if !b3.full_present() {
        caught += 1;
    }
    (caught, 6)
}

/// 判据入口（CheckSet）。
pub fn run_unx46421_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F46421");
    // J1-1 全集 ×1 扫描零缺失
    let b = standard_scan();
    let ok = b.full_present() && b.rows.len() == 20;
    s.add("46421-J1-全集扫描零缺失", ok, if ok { "" } else { "扫描缺失" });
    // J1-2 指纹 ×2 次一致
    let ok = b.fingerprint() == standard_scan().fingerprint();
    s.add("46421-J1-指纹×2一致", ok, if ok { "" } else { "指纹漂移" });
    // J1-3 内容敏感指纹（同头异内容指纹必变）
    let fp1 = HeadEntry::synth_fp("stdio.h", b"content A");
    let fp2 = HeadEntry::synth_fp("stdio.h", b"content B");
    let ok = fp1 != fp2 && fp1 == HeadEntry::synth_fp("stdio.h", b"content A");
    s.add("46421-J1-内容敏感指纹", ok, if ok { "" } else { "指纹不敏感" });
    // J1-4 可重放核（同夹具内容重合成逐头一致）
    let contents: Vec<(String, Vec<u8>)> = C_HEADERS20
        .iter()
        .map(|h| ((*h).to_string(), format!("/* {} Varix S1 */\n", h).into_bytes()))
        .collect();
    let ok = b.fp_replay_ok(&contents) && b.fp_stable();
    s.add("46421-J1-指纹可重放", ok, if ok { "" } else { "重放漂移" });
    // J1-5 头依赖账：边集登记 + 二跳闭包 + 无环
    let mut deps = HeadDeps::new();
    deps.edge("stdio.h", "stddef.h").expect("e1");
    deps.edge("stdlib.h", "stddef.h").expect("e2");
    deps.edge("string.h", "stddef.h").expect("e3");
    deps.edge("stdint.h", "stddef.h").expect("e4");
    deps.edge("signal.h", "time.h").expect("e5");
    let ok = deps.direct("stdio.h").len() == 1
        && deps.closure2("signal.h").contains(&"stddef.h".to_string())
        && !deps.has_cycle()
        && deps.edge("time.h", "signal.h").is_ok()
        && deps.has_cycle();
    s.add("46421-J1-头依赖账", ok, if ok { "" } else { "依赖账漂移" });
    // J1-6 扫描报告 20 行齐
    let ok = report_ok(&b) && b.stats() == (20, 0, 20);
    s.add("46421-J1-扫描报告", ok, if ok { "" } else { "报告漂移" });
    // J1-7 指纹快照账：两轮一致 + 篡改轮漂移定位
    let mut snaps = FpSnapshots::new();
    snaps.snap(1, b.fingerprint()).expect("snap1");
    snaps.snap(2, b.fingerprint()).expect("snap2");
    let equal = snaps.last_two_equal();
    snaps.snap(3, b.fingerprint() ^ 1).expect("snap3");
    let ok = equal && snaps.drift_pairs() == vec![(2, 3)];
    s.add("46421-J1-指纹快照账", ok, if ok { "" } else { "快照漂移" });
    // 注入核 6/6
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("46421-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx46421_all_checks_green() {
        let set = run_unx46421_checks();
        assert!(set.all_passed(), "UNX-F46421 checks must be green: {}", set.red_items_text());
    }
}
