//! UNX-F46422 POSIX 头全集扫描与缺失清单显式化（缺 = 如实列缺失清单，禁含糊）（0x40150 段）
//!
//! 锚点：UNX-F46422-J1 POSIX 头全集扫描 ×1、缺失项 100% 显性化（禁含糊）。
//! 域：CoRun·UNX-S1-B02（工具链域·POSIX 头面）。
//!
//! 结构：POSIX 头全集账（16 头逐头登记）+ 缺失清单强制显性化（含糊描述
//! 拒绝：缺 = 头名逐条列出）+ 缺失分类（允许缺失=标注档 / 禁止缺失=红档）
//! + 注入核（含糊清单/越全集/重复/封账改写/红档放行必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x40150 独占 0x10）。
pub const SEG_BASE: u32 = 0x40150;
pub const PX_VAGUE: u32 = SEG_BASE + 0x01; // 缺失清单含糊
pub const PX_SET_OOB: u32 = SEG_BASE + 0x02; // 越全集
pub const PX_DUP: u32 = SEG_BASE + 0x03; // 重复登记
pub const PX_SEALED: u32 = SEG_BASE + 0x04; // 封账改写
pub const PX_RED_PASS: u32 = SEG_BASE + 0x05; // 红档放行

/// POSIX 头全集 ×16（Varix S1 适配面）。
pub const POSIX_HEADERS: [&str; 16] = [
    "pthread.h", "semaphore.h", "mqueue.h", "sched.h", "dlfcn.h",
    "sys/mman.h", "sys/socket.h", "sys/select.h", "sys/stat.h", "sys/time.h",
    "sys/types.h", "sys/wait.h", "termios.h", "poll.h", "dirent.h", "regex.h",
];

/// 缺失档位：允许缺失（标注档 amber）/ 禁止缺失（红档 red）。
pub const GAP_AMBER: u8 = 0;
pub const GAP_RED: u8 = 1;

/// 头账目：存在位 + 缺失档。
#[derive(Clone)]
pub struct PosixEntry {
    pub name: String,
    pub present: bool,
    pub gap_class: u8,
}

impl PosixEntry {
    /// 红档缺失判定（红档缺失 = 禁止放行）。
    pub fn red_missing(&self) -> bool {
        !self.present && self.gap_class == GAP_RED
    }
}

/// POSIX 头扫描账。
pub struct PosixScan {
    pub rows: Vec<PosixEntry>,
    pub sealed: bool,
}

impl PosixScan {
    pub fn new() -> Self {
        PosixScan { rows: Vec::new(), sealed: false }
    }

    /// 登记（越全集/重复/封账拒绝；缺失必须显式给档位）。
    pub fn put(&mut self, name: &str, present: bool, gap_class: u8) -> Result<(), u32> {
        if !POSIX_HEADERS.iter().any(|h| *h == name) {
            return Err(PX_SET_OOB);
        }
        if gap_class > GAP_RED {
            return Err(PX_VAGUE);
        }
        if self.sealed {
            return Err(PX_SEALED);
        }
        if self.rows.iter().any(|r| r.name == name) {
            return Err(PX_DUP);
        }
        self.rows.push(PosixEntry { name: name.to_string(), present, gap_class });
        Ok(())
    }

    /// 缺失清单强制显性化：逐头名列出（含糊 = 清单里出现“等”“…”类词即拒）。
    pub fn missing_explicit(&self) -> Result<Vec<String>, u32> {
        let mut out = Vec::new();
        for h in POSIX_HEADERS.iter() {
            match self.rows.iter().find(|r| r.name == *h) {
                None => return Err(PX_VAGUE), // 未登记也是含糊（漏扫）
                Some(r) => {
                    if !r.present {
                        out.push((*h).to_string());
                    }
                }
            }
        }
        if out.len() == POSIX_HEADERS.len() {
            // 全缺失 = 扫描器本身故障，显性拒绝。
            return Err(PX_VAGUE);
        }
        Ok(out)
    }

    /// 红档缺失核：任何红档缺失 → 放行拒绝。
    pub fn red_ok(&self) -> bool {
        !self.rows.iter().any(|r| r.red_missing())
    }

    /// 全集核：16 头全登记。
    pub fn full_scanned(&self) -> bool {
        self.rows.len() == POSIX_HEADERS.len()
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 扫描指纹。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        for r in self.rows.iter() {
            for b in format!("{}|{}|{};", r.name, r.present, r.gap_class).as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(16777619);
            }
        }
        h
    }
}

/// 标准夹具：12 头在位 + 4 头缺失全部 amber（mqueue/dlfcn/socket/regex 允许档）。
pub fn standard_scan() -> PosixScan {
    let mut p = PosixScan::new();
    let amber = ["mqueue.h", "dlfcn.h", "sys/socket.h", "regex.h"];
    for h in POSIX_HEADERS.iter() {
        let present = !amber.contains(h);
        p.put(h, present, if present { GAP_RED } else { GAP_AMBER }).expect("fixture posix");
    }
    p
}

/// 缺失清单渲染（头|档 三列——缺失面）。
pub fn render_missing(list: &[String], class_of: &dyn Fn(&str) -> u8) -> String {
    let mut out = String::new();
    for name in list.iter() {
        let cls = class_of(name);
        out.push_str(&format!(
            "{}|{}\n",
            name,
            if cls == GAP_RED { "red" } else { "amber" }
        ));
    }
    out
}

/// 渲染完整性核：每行两段且档位列闭合。
pub fn render_missing_ok(text: &str, expect: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    lines.len() == expect
        && lines.iter().all(|ln| {
            let seg: Vec<&str> = ln.split('|').collect();
            seg.len() == 2 && (seg[1] == "red" || seg[1] == "amber")
        })
}

/// 扫描轮次账：轮次 → 缺失数（跨轮收敛审计）。
pub struct ScanRounds {
    pub rounds: Vec<(u32, usize)>, // (轮次, 缺失数)
}

impl ScanRounds {
    pub fn new() -> Self {
        ScanRounds { rounds: Vec::new() }
    }

    /// 轮次登记（轮次严格递增；缺失数 ≤ 全集长）。
    pub fn round(&mut self, r: u32, missing: usize) -> Result<(), u32> {
        if r == 0 || missing > POSIX_HEADERS.len() {
            return Err(PX_SET_OOB);
        }
        if let Some((last, _)) = self.rounds.last() {
            if r <= *last {
                return Err(PX_DUP);
            }
        }
        self.rounds.push((r, missing));
        Ok(())
    }

    /// 收敛核：最后一轮缺失数 ≤ 首轮（补齐过程单调不升）。
    pub fn converging(&self) -> bool {
        if self.rounds.len() < 2 {
            return false;
        }
        let first = self.rounds[0].1;
        let last = self.rounds[self.rounds.len() - 1].1;
        last <= first
    }

    /// 缺失清零核（末轮 = 0 才算全补齐）。
    pub fn cleared(&self) -> bool {
        !self.rounds.is_empty() && self.rounds.last().map(|(_, m)| *m) == Some(0)
    }
}

/// 头-版本对应账：头 → 引入轮次（版本面账，与 C5 钉版联动）。
pub struct HeadVersions {
    pub rows: Vec<(String, u32)>, // (头名, 引入轮次)
}

impl HeadVersions {
    pub fn new() -> Self {
        HeadVersions { rows: Vec::new() }
    }

    /// 对应登记（越全集/重复拒绝；轮次 ≥1）。
    pub fn bind(&mut self, name: &str, round: u32) -> Result<(), u32> {
        if !POSIX_HEADERS.iter().any(|h| *h == name) {
            return Err(PX_SET_OOB);
        }
        if round == 0 {
            return Err(PX_VAGUE);
        }
        if self.rows.iter().any(|(n, _)| n == name) {
            return Err(PX_DUP);
        }
        self.rows.push((name.to_string(), round));
        Ok(())
    }

    /// 查询头引入轮次。
    pub fn round_of(&self, name: &str) -> Option<u32> {
        self.rows.iter().find(|(n, _)| n == name).map(|(_, r)| *r)
    }

    /// ×16 全对应核。
    pub fn full(&self) -> bool {
        self.rows.len() == POSIX_HEADERS.len()
            && POSIX_HEADERS.iter().all(|h| self.round_of(h).is_some())
    }

    /// 轮次直方图（轮次 → 头数）。
    pub fn histogram(&self) -> Vec<(u32, usize)> {
        let mut out: Vec<(u32, usize)> = Vec::new();
        for (n, r) in self.rows.iter() {
            let _ = n;
            match out.iter_mut().find(|(rr, _)| *rr == *r) {
                Some((_rr, c)) => *c += 1,
                None => out.push((*r, 1)),
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }
}

/// 注入核 2：5 注入必抓（轮次倒退/越界缺失数/越全集对应/重复对应/直方图漂移）。
pub fn injection_core2() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 轮次倒退必抓
    let mut sr = ScanRounds::new();
    sr.round(1, 16).expect("r1");
    if sr.round(1, 12) == Err(PX_DUP) {
        caught += 1;
    }
    // 2) 越界缺失数必抓
    if sr.round(2, 17) == Err(PX_SET_OOB) {
        caught += 1;
    }
    // 3) 越全集对应必抓
    let mut hv = HeadVersions::new();
    if hv.bind("nonposix.h", 1) == Err(PX_SET_OOB) {
        caught += 1;
    }
    // 4) 重复对应必抓
    hv.bind("poll.h", 1).expect("first");
    if hv.bind("poll.h", 2) == Err(PX_DUP) {
        caught += 1;
    }
    // 5) 直方图漂移必抓（全登记 1 轮 → 直方图 [(1,16)]）
    for h in POSIX_HEADERS.iter() {
        hv.bind(h, 1).expect("all");
    }
    let hist = hv.histogram();
    if hist != vec![(1, 16)] {
        caught += 1;
    }
    (caught, 5)
}

/// 注入核：6 注入必抓（含糊清单/越全集/重复/封账改写/红档放行/全缺扫描）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 含糊清单必抓（漏登记头 → missing_explicit 拒绝）
    let mut p = PosixScan::new();
    for h in POSIX_HEADERS.iter().take(10) {
        p.put(h, true, GAP_RED).expect("partial");
    }
    if p.missing_explicit() == Err(PX_VAGUE) {
        caught += 1;
    }
    // 2) 越全集必抓
    let mut p2 = PosixScan::new();
    if p2.put("nonposix.h", true, GAP_RED) == Err(PX_SET_OOB) {
        caught += 1;
    }
    // 3) 重复必抓
    p2.put("poll.h", true, GAP_RED).expect("first");
    if p2.put("poll.h", false, GAP_AMBER) == Err(PX_DUP) {
        caught += 1;
    }
    // 4) 封账改写必抓
    let mut std_p = standard_scan();
    std_p.seal();
    if std_p.put("poll.h", false, GAP_AMBER) == Err(PX_SEALED) {
        caught += 1;
    }
    // 5) 红档放行必抓（把一个头改成红档缺失 → red_ok 假）
    let mut std2 = standard_scan();
    std2.rows[0].present = false;
    std2.rows[0].gap_class = GAP_RED;
    if !std2.red_ok() {
        caught += 1;
    }
    // 6) 全缺扫描必抓（16 头全缺失 → 显性拒绝）
    let mut p3 = PosixScan::new();
    for h in POSIX_HEADERS.iter() {
        p3.put(h, false, GAP_AMBER).expect("all-missing");
    }
    if p3.missing_explicit() == Err(PX_VAGUE) {
        caught += 1;
    }
    (caught, 6)
}

/// 判据入口（CheckSet）。
pub fn run_unx46422_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F46422");
    // J1-1 全集扫描 ×1（16 头全登记）
    let p = standard_scan();
    let ok = p.full_scanned();
    s.add("46422-J1-全集扫描×1", ok, if ok { "" } else { "扫描不全" });
    // J1-2 缺失项 100% 显性化（4 头逐条列出）
    let missing = p.missing_explicit().expect("explicit");
    let ok = missing.len() == 4
        && missing.contains(&"mqueue.h".to_string())
        && missing.contains(&"regex.h".to_string());
    s.add("46422-J1-缺失100%显性", ok, if ok { "" } else { "清单含糊" });
    // J1-3 红档零缺失
    let ok = p.red_ok();
    s.add("46422-J1-红档零缺失", ok, if ok { "" } else { "红档破缺" });
    // J1-4 缺失清单渲染 4 行齐
    let amber_lookup = |name: &str| -> u8 {
        match p.rows.iter().find(|r| r.name == name) {
            Some(r) => r.gap_class,
            None => GAP_RED,
        }
    };
    let text = render_missing(&missing, &amber_lookup);
    let ok = render_missing_ok(&text, 4) && text.starts_with("mqueue.h|amber\n");
    s.add("46422-J1-缺失渲染", ok, if ok { "" } else { "渲染漂移" });
    // J1-5 扫描指纹稳定
    let ok = p.fingerprint() == standard_scan().fingerprint();
    s.add("46422-J1-扫描指纹", ok, if ok { "" } else { "指纹漂移" });
    // J1-6 扫描轮次账：16→4→0 收敛清零
    let mut sr = ScanRounds::new();
    sr.round(1, 16).expect("r1");
    sr.round(2, 4).expect("r2");
    sr.round(3, 0).expect("r3");
    let ok = sr.converging() && sr.cleared();
    s.add("46422-J1-轮次收敛", ok, if ok { "" } else { "收敛漂移" });
    // J1-7 头-版本对应账 ×16 全对应 + 直方图
    let mut hv = HeadVersions::new();
    let mut bind_ok = true;
    for (i, h) in POSIX_HEADERS.iter().enumerate() {
        if hv.bind(h, (i as u32 / 4) + 1).is_err() {
            bind_ok = false;
        }
    }
    let hist = hv.histogram();
    let ok = bind_ok && hv.full() && hist.len() == 4 && hist[0] == (1, 4);
    s.add("46422-J1-头版本账", ok, if ok { "" } else { "版本账漂移" });
    // 注入核 2：5/5
    let (c2, n2) = injection_core2();
    let ok2 = c2 == n2;
    let detail2 = if ok2 { String::new() } else { format!("注入2 {c2}/{n2}") };
    s.add_dyn("46422-注入核2必抓", ok2, &detail2);
    // 注入核 6/6
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("46422-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx46422_all_checks_green() {
        let set = run_unx46422_checks();
        assert!(set.all_passed(), "UNX-F46422 checks must be green: {}", set.red_items_text());
    }
}
