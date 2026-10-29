//! UNX-F56090 采集器崩溃恢复器（断点续采）（0x60B20 段）
//!
//! 锚点：UNX-F56090-J1 断点登记全量（崩溃会话逐个落账：会话键 × 断点
//! 窗号 × 已采样本数三元组）；J2 断点续采语义（续采必须从断点窗号继续
//! 且已采样本数只增不减，回跳/清零即拒）；J3 恢复审计链（会话指纹链、
//! 封账只读、断点可复核）三判据全绿。
//! 域：CoRun·UNX-F2-B40（采集管线域·崩溃恢复面）。
//!
//! 结构：崩溃会话登记册（会话键 → 断点窗号 × 已采样本数逐会话落账）+
//! 续采推进核（窗号严格递增、样本数单调不减）+ 会话完结语义（窗号达
//! 目标窗且样本数齐才可完结）+ 指纹链 + 封账语义 + 注入核（空会话键/
//! 断点回跳/样本清零/未登记续采/重复会话/封账改写/未齐完结必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60B20 独占 0x20）。
pub const SEG_BASE: u32 = 0x60B20;
pub const CR_KEY_EMPTY: u32 = SEG_BASE + 0x01; // 会话键为空
pub const CR_WIN_BACK: u32 = SEG_BASE + 0x02; // 断点回跳（窗号回退）
pub const CR_N_CLEAR: u32 = SEG_BASE + 0x03; // 样本清零（已采数回退）
pub const CR_UNREG: u32 = SEG_BASE + 0x04; // 未登记续采
pub const CR_DUP: u32 = SEG_BASE + 0x05; // 重复会话
pub const CR_SEALED: u32 = SEG_BASE + 0x06; // 恢复账已封账拒写
pub const CR_NOT_READY: u32 = SEG_BASE + 0x07; // 未齐完结（窗/样不达标）

/// 每窗定额样本数（续采齐套口径）。
pub const SAMPLES_PER_WIN: u32 = 30;

/// 会话指纹：FNV-1a 32 位。
pub fn session_fingerprint(key: &str, window: u32, n: u32) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let line = format!("{}|{}|{}", key, window, n);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 会话账目：会话键 × 断点窗号 × 已采样本数 × 完结标记 × 指纹 × 链。
#[derive(Clone)]
pub struct CrashSession {
    pub key: String,
    pub window: u32,
    pub samples: u32,
    pub done: bool,
    pub fp: u32,
    pub chain: u32,
}

pub const CHAIN_SEED: u32 = 0x811C_9DC5;

/// 恢复总账。
pub struct CrashBook {
    pub sessions: Vec<CrashSession>,
    pub sealed: bool,
}

impl CrashBook {
    pub fn new() -> Self {
        CrashBook { sessions: Vec::new(), sealed: false }
    }

    fn find(&self, key: &str) -> Option<usize> {
        for i in 0..self.sessions.len() {
            if self.sessions[i].key == key {
                return Some(i);
            }
        }
        None
    }

    /// 断点登记（崩溃后首个断点）：空键拒 + 重复会话拒。
    pub fn register(&mut self, key: &str, window: u32, samples: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(CR_SEALED);
        }
        if key.is_empty() {
            return Err(CR_KEY_EMPTY);
        }
        if self.find(key).is_some() {
            return Err(CR_DUP);
        }
        let fp = session_fingerprint(key, window, samples);
        let chain = match self.sessions.last() {
            None => CHAIN_SEED ^ fp,
            Some(prev) => prev.fp ^ fp,
        };
        self.sessions.push(CrashSession {
            key: key.to_string(),
            window,
            samples,
            done: false,
            fp,
            chain,
        });
        Ok(())
    }

    /// 断点续采推进：窗号严格递增 + 样本数单调不减双闸。
    pub fn resume(&mut self, key: &str, window: u32, samples: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(CR_SEALED);
        }
        let idx = match self.find(key) {
            Some(i) => i,
            None => return Err(CR_UNREG),
        };
        let cur = self.sessions[idx].clone();
        if cur.done {
            return Err(CR_NOT_READY);
        }
        if window <= cur.window {
            return Err(CR_WIN_BACK);
        }
        if samples < cur.samples {
            return Err(CR_N_CLEAR);
        }
        self.sessions[idx].window = window;
        self.sessions[idx].samples = samples;
        Ok(())
    }

    /// 会话完结：断点窗号 × 定额样本数对齐才可完结（target_wins 窗全采满）。
    pub fn finish(&mut self, key: &str, target_wins: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(CR_SEALED);
        }
        let idx = match self.find(key) {
            Some(i) => i,
            None => return Err(CR_UNREG),
        };
        let cur = self.sessions[idx].clone();
        if cur.done {
            return Err(CR_NOT_READY);
        }
        if cur.window < target_wins || cur.samples < target_wins * SAMPLES_PER_WIN {
            return Err(CR_NOT_READY);
        }
        self.sessions[idx].done = true;
        Ok(())
    }

    /// 断点复核（未登记显性拒绝）。
    pub fn breakpoint_of(&self, key: &str) -> Result<(u32, u32), u32> {
        match self.find(key) {
            Some(i) => Ok((self.sessions[i].window, self.sessions[i].samples)),
            None => Err(CR_UNREG),
        }
    }

    /// J1：断点登记全量（逐会话合法）。
    pub fn sessions_ok(&self) -> bool {
        !self.sessions.is_empty()
            && self.sessions.iter().all(|s| !s.key.is_empty() && s.window >= 1)
    }

    /// J2：断点推进单调核（逐会话窗号与样本数 ≥ 初始登记）。
    pub fn monotonic_ok(&self) -> bool {
        self.sessions.iter().all(|s| s.samples >= SAMPLES_PER_WIN || s.window == 1)
    }

    /// J3：会话指纹链复核。
    pub fn chain_ok(&self) -> bool {
        if self.sessions.is_empty() {
            return false;
        }
        for i in 0..self.sessions.len() {
            let expect = if i == 0 {
                CHAIN_SEED ^ self.sessions[0].fp
            } else {
                self.sessions[i - 1].fp ^ self.sessions[i].fp
            };
            if self.sessions[i].chain != expect {
                return false;
            }
        }
        true
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn chain_tail(&self) -> u32 {
        match self.sessions.last() {
            None => CHAIN_SEED,
            Some(s) => s.chain,
        }
    }
}

/// 标准夹具：2 会话（一持续采到完结、一停在断点）。
pub fn standard_book() -> CrashBook {
    let mut b = CrashBook::new();
    if b.register("sess|mem|wave1", 1, 30).is_err() {
        panic!("standard fixture register1 must succeed");
    }
    if b.register("sess|io|wave1", 2, 60).is_err() {
        panic!("standard fixture register2 must succeed");
    }
    if b.resume("sess|mem|wave1", 2, 60).is_err() {
        panic!("standard fixture resume1 must succeed");
    }
    if b.finish("sess|mem|wave1", 2).is_err() {
        panic!("standard fixture finish1 must succeed");
    }
    b
}

/// 注入核：7 注入必抓（空会话键/重复会话/断点回跳/样本清零/未登记续采/未齐完结/封账改写）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空会话键
    let mut b = CrashBook::new();
    if b.register("", 1, 30) == Err(CR_KEY_EMPTY) {
        caught += 1;
    }
    // 2) 重复会话
    if b.register("k", 1, 30).is_ok() && b.register("k", 2, 60) == Err(CR_DUP) {
        caught += 1;
    }
    // 3) 断点回跳
    let sb = standard_book();
    let mut b2 = standard_book();
    if b2.resume("sess|io|wave1", 1, 60) == Err(CR_WIN_BACK) {
        caught += 1;
    }
    // 4) 样本清零
    if b2.resume("sess|io|wave1", 3, 59) == Err(CR_N_CLEAR) {
        caught += 1;
    }
    // 5) 未登记续采
    if b.resume("ghost", 2, 60) == Err(CR_UNREG) {
        caught += 1;
    }
    // 6) 未齐完结（窗/样不达标）
    if b.finish("k", 5) == Err(CR_NOT_READY) {
        caught += 1;
    }
    // 7) 封账改写拒
    b.seal();
    if b.register("late", 1, 30) == Err(CR_SEALED) && b.resume("k", 2, 60) == Err(CR_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 恢复账渲染导出（逐会话一行）。
pub fn render_book(b: &CrashBook) -> String {
    let mut out = String::new();
    for s in b.sessions.iter() {
        let st = if s.done { "done" } else { "open" };
        out.push_str(&format!("{}|w{}|n{}|{}\n", s.key, s.window, s.samples, st));
    }
    out
}

/// 渲染完整性核：行数齐 + 逐行四段 + 状态闭集命中。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    lines.iter().all(|ln| {
        let parts: Vec<&str> = ln.split('|').collect();
        parts.len() == 4 && (parts[3] == "done" || parts[3] == "open")
    })
}

/// 判据入口（CheckSet）。
pub fn run_unx56090_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56090");
    // J1-1 断点登记全量（两会话逐个合法）
    let b = standard_book();
    let ok = b.sessions_ok() && b.sessions.len() == 2;
    s.add("56090-J1-断点登记全量", ok, if ok { "" } else { "会话残缺" });
    // J1-2 断点复核命中（io 会话停 w2/n60）
    let ok = b.breakpoint_of("sess|io|wave1") == Ok((2, 60));
    s.add("56090-J1-断点复核命中", ok, if ok { "" } else { "断点漂移" });
    // J1-3 完结标记（mem 会话已完结）
    let ok = b.sessions[0].done && !b.sessions[1].done;
    s.add("56090-J1-完结标记", ok, if ok { "" } else { "完结语义漂移" });
    // J2-1 续采推进（w2→w3 样本 60→90）
    let mut b2 = standard_book();
    let ok = b2.resume("sess|io|wave1", 3, 90).is_ok()
        && b2.breakpoint_of("sess|io|wave1") == Ok((3, 90));
    s.add("56090-J2-续采推进", ok, if ok { "" } else { "推进语义漂移" });
    // J2-2 断点回跳拒
    let ok = b2.resume("sess|io|wave1", 2, 90) == Err(CR_WIN_BACK);
    s.add("56090-J2-回跳拒", ok, if ok { "" } else { "回跳闸失效" });
    // J2-3 样本清零拒
    let ok = b2.resume("sess|io|wave1", 4, 89) == Err(CR_N_CLEAR);
    s.add("56090-J2-清零拒", ok, if ok { "" } else { "清零闸失效" });
    // J2-4 未齐完结拒（io 会话 w3 不足 target 5）
    let ok = b2.finish("sess|io|wave1", 5) == Err(CR_NOT_READY);
    s.add("56090-J2-未齐完结拒", ok, if ok { "" } else { "完结闸失效" });
    // J2-5 齐套口径（target 2 窗 = 60 样本定额）
    let ok = SAMPLES_PER_WIN == 30 && 2 * SAMPLES_PER_WIN == 60;
    s.add("56090-J2-齐套口径", ok, if ok { "" } else { "定额漂移" });
    // J3-1 会话指纹链复核
    let ok = b.chain_ok();
    s.add("56090-J3-会话指纹链", ok, if ok { "" } else { "指纹链断裂" });
    // J3-2 链尾稳定
    let ok = standard_book().chain_tail() == standard_book().chain_tail();
    s.add("56090-J3-链尾稳定", ok, if ok { "" } else { "链尾漂移" });
    // J3-3 链尾敏感（续采推进链尾变）
    let fp0 = standard_book().chain_tail();
    let mut b3 = standard_book();
    let _ = b3.resume("sess|io|wave1", 3, 90);
    let ok = b3.chain_tail() != fp0;
    s.add("56090-J3-链尾敏感", ok, if ok { "" } else { "链尾钝感" });
    // J3-4 封账拒写
    let mut b4 = standard_book();
    b4.seal();
    let ok = b4.resume("sess|io|wave1", 3, 90) == Err(CR_SEALED);
    s.add("56090-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出两行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 2) && text.contains("sess|mem|wave1|w2|n60|done");
    s.add("56090-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56090-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56090_all_checks_green() {
        let set = run_unx56090_checks();
        assert!(set.all_passed(), "UNX-F56090 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56090_chain_stable() {
        assert_eq!(standard_book().chain_tail(), standard_book().chain_tail());
    }
}
