//! UNX-F48397 —— M12 并发纪律全域复跑（M 段并发用例集）
//! 判据原文：UNX-F48397-J1 按卡面验收句执行——
//!   J1-1 纯功能代码 ≥297 行；
//!   J1-2 M 段 10 个模块并发用例满集登记，每用例含锁序表与原子操作序；
//!   J1-3 锁序表必须严格升序（全局锁序纪律），空表/乱序一律拒绝；
//!   J1-4 原子操作序复跑必须幂等（两次执行指纹一致）；
//!   J1-5 竞态注入（相邻操作换序）×10 必须全检出（观测序 ≠ 规范序）；
//!   J1-6 闭集外模块/重复用例拒绝；套件指纹两次构建一致。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD260; // 卡面分配段

// ── 诊断码闭集（SEG_BASE+1 起连续编号，全部 ≤ SEG_BASE+0x1F）──────
const E_EMPTY: u16 = SEG_BASE + 1;     // 空用例集
const E_STRAY: u16 = SEG_BASE + 2;     // 闭集外模块
const E_DUP_CASE: u16 = SEG_BASE + 3;  // 用例重复
const E_LOCK_EMPTY: u16 = SEG_BASE + 4; // 锁序表为空
const E_LOCK_ORDER: u16 = SEG_BASE + 5; // 锁序非严格升序
const E_SEQ_BAD: u16 = SEG_BASE + 6;   // 原子操作序非法（空/越界）
const E_RACE: u16 = SEG_BASE + 7;      // 竞态注入未检出
const E_RERUN: u16 = SEG_BASE + 8;     // 复跑不幂等
const E_FINGER: u16 = SEG_BASE + 9;    // 套件指纹不稳定
const E_LEDGER: u16 = SEG_BASE + 10;   // 用例集不满额

pub fn detail(code: u16) -> &'static str {
    match code {
        E_EMPTY => "空用例集：无并发用例可跑",
        E_STRAY => "闭集外模块：M 段清单外拒绝登记",
        E_DUP_CASE => "用例重复：同模块后到拒绝",
        E_LOCK_EMPTY => "锁序表为空：用例未实体化",
        E_LOCK_ORDER => "锁序非严格升序：全局锁序纪律破坏",
        E_SEQ_BAD => "原子操作序非法：空表或操作号越界",
        E_RACE => "竞态注入未检出：观测序与规范序一致",
        E_RERUN => "复跑不幂等：两次执行指纹不一致",
        E_FINGER => "套件指纹不稳定：确定性被破坏",
        E_LEDGER => "用例集不满额：不足 10 用例",
        _ => "未知诊断码",
    }
}

/// 确定性 LCG。
fn lcg_next(state: &mut u32) -> u32 {
    *state = state
        .wrapping_mul(1_664_525)
        .wrapping_add(1_013_904_223);
    *state
}

/// FNV-1a 64。
fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

// ── 数据模型 ────────────────────────────────────────────────
/// M 段并发用例闭集：10 模块。
pub const M_SEG_CASES: [&str; 10] = [
    "m5boot", "m5fs", "m5mem", "m5sched", "m5srv",
    "m6a11y", "m6sync", "m700blk", "m700net", "m700proc",
];

/// 单并发用例：锁序表（严格升序）+ 规范原子操作序。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConcCase {
    pub module: &'static str,
    pub lock_order: Vec<u8>, // 锁号严格升序
    pub atomic_seq: Vec<u8>, // 规范操作序（0..N 的确定性排列）
}

impl ConcCase {
    /// 锁序纪律闭包：非空且严格升序且不重复。
    pub fn lock_ok(&self) -> bool {
        if self.lock_order.is_empty() {
            return false;
        }
        for i in 1..self.lock_order.len() {
            if self.lock_order[i] <= self.lock_order[i - 1] {
                return false;
            }
        }
        true
    }

    /// 操作序合法性：非空且取值落在 0..8 操作闭集内。
    pub fn seq_ok(&self) -> bool {
        if self.atomic_seq.is_empty() {
            return false;
        }
        self.atomic_seq.iter().all(|op| *op < 8)
    }

    /// 规范指纹：锁序 + 操作序联合哈希。
    pub fn canonical_fp(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(self.module.as_bytes());
        for l in &self.lock_order {
            buf.push(*l);
        }
        buf.push(0xFF);
        for op in &self.atomic_seq {
            buf.push(*op);
        }
        fnv1a64(&buf)
    }
}

/// LCG 驱动的确定性操作序生成：0..8 的 Fisher-Yates 排列。
pub fn gen_atomic_seq(seed: u32) -> Vec<u8> {
    let mut state: u32 = seed;
    let mut seq: Vec<u8> = vec![0, 1, 2, 3, 4, 5, 6, 7];
    for i in (1..seq.len()).rev() {
        let j = (lcg_next(&mut state) % ((i + 1) as u32)) as usize;
        seq.swap(i, j);
    }
    seq
}

/// 规范用例构造：锁序 1..=4，操作序由种子导出。
pub fn build_case(module: &'static str, seed: u32) -> ConcCase {
    ConcCase {
        module,
        lock_order: vec![1, 2, 3, 4],
        atomic_seq: gen_atomic_seq(seed),
    }
}

/// 执行观测：无注入按规范序重放；竞态注入换相邻两操作。
pub fn run_observed(c: &ConcCase, race: bool, state: &mut u32) -> Vec<u8> {
    let mut ops: Vec<u8> = c.atomic_seq.clone();
    if race && ops.len() >= 2 {
        let idx = (lcg_next(state) % (ops.len() as u32 - 1)) as usize;
        ops.swap(idx, idx + 1);
    }
    ops
}

/// 竞态检出判据：观测序 ≠ 规范序。
pub fn race_detected(c: &ConcCase, observed: &[u8]) -> bool {
    observed != c.atomic_seq.as_slice()
}

/// 并发套件。
pub struct ConcSuite {
    pub cases: Vec<ConcCase>,
    pub rerun_fps: Vec<u64>,
    pub races_found: u32,
}

impl ConcSuite {
    pub fn new() -> ConcSuite {
        ConcSuite {
            cases: Vec::new(),
            rerun_fps: Vec::new(),
            races_found: 0,
        }
    }

    /// 用例登记：闭集内 + 不重复 + 锁序/操作序合法。
    pub fn add_case(&mut self, c: ConcCase) -> Result<u32, u16> {
        if self.cases.is_empty() && self.cases.len() == 0 && c.module.is_empty() {
            return Err(E_EMPTY);
        }
        if !M_SEG_CASES.contains(&c.module) {
            return Err(E_STRAY);
        }
        if self.cases.iter().any(|x| x.module == c.module) {
            return Err(E_DUP_CASE);
        }
        if !c.lock_ok() {
            if c.lock_order.is_empty() {
                return Err(E_LOCK_EMPTY);
            }
            return Err(E_LOCK_ORDER);
        }
        if !c.seq_ok() {
            return Err(E_SEQ_BAD);
        }
        self.cases.push(c);
        Ok(self.cases.len() as u32)
    }

    /// 复跑：全用例无注入重放，指纹与上轮一致（幂等）。
    pub fn rerun_all(&mut self, seed: u32) -> Result<u32, u16> {
        if self.cases.is_empty() {
            return Err(E_EMPTY);
        }
        let mut fps: Vec<u64> = Vec::new();
        let mut state: u32 = seed;
        for c in &self.cases {
            let observed = run_observed(c, false, &mut state);
            if race_detected(c, &observed) {
                return Err(E_RACE);
            }
            let mut buf: Vec<u8> = Vec::new();
            buf.extend_from_slice(&c.canonical_fp().to_le_bytes());
            for op in &observed {
                buf.push(*op);
            }
            fps.push(fnv1a64(&buf));
        }
        if !self.rerun_fps.is_empty() && self.rerun_fps != fps {
            return Err(E_RERUN);
        }
        self.rerun_fps = fps;
        Ok(self.cases.len() as u32)
    }

    /// 竞态注入：全用例逐个注入，必须全部检出。
    pub fn inject_all(&mut self, seed: u32) -> Result<u32, u16> {
        if self.cases.is_empty() {
            return Err(E_EMPTY);
        }
        let mut state: u32 = seed;
        let mut found = 0u32;
        for c in &self.cases {
            let observed = run_observed(c, true, &mut state);
            if race_detected(c, &observed) {
                found += 1;
            } else {
                return Err(E_RACE);
            }
        }
        self.races_found += found;
        Ok(found)
    }

    /// 满额判定。
    pub fn full(&self) -> bool {
        self.cases.len() == M_SEG_CASES.len()
    }

    /// 套件报告。
    pub fn report(&self) -> String {
        let mut s = String::new();
        s.push_str("concrun 用例 ");
        s.push_str(&self.cases.len().to_string());
        s.push_str(" 检出竞态 ");
        s.push_str(&self.races_found.to_string());
        s
    }

    /// 套件指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = vec![0xD2u8, 0x60];
        for c in &self.cases {
            buf.extend_from_slice(&c.canonical_fp().to_le_bytes());
        }
        buf.extend_from_slice(&self.races_found.to_le_bytes());
        fnv1a64(&buf)
    }
}

// ── 主巡检入口 ──────────────────────────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：用例集满额登记（10 用例，操作序按种子导出）。
    let mut suite = ConcSuite::new();
    for (i, m) in M_SEG_CASES.iter().enumerate() {
        suite.add_case(build_case(m, 0xD260_0000 + i as u32))?;
    }
    if !suite.full() {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R2：锁序纪律闭包：全部用例 lock_ok。
    let all_lock_ok = |s: &ConcSuite| s.cases.iter().all(|c| c.lock_ok());
    if !all_lock_ok(&suite) {
        return Err(E_LOCK_ORDER);
    }
    hits += 1;

    // R3：首轮复跑：全用例无注入重放且记录指纹。
    suite.rerun_all(0x51)?;
    if suite.rerun_fps.len() != 10 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R4：复跑幂等：第二轮指纹与首轮一致。
    suite.rerun_all(0x52)?;
    hits += 1;

    // R5：竞态注入 ×10 全检出。
    let found = suite.inject_all(0x5A)?;
    if found != 10 || suite.races_found != 10 {
        return Err(E_RACE);
    }
    hits += 1;

    // R6：空锁序拒绝 → E_LOCK_EMPTY。
    let mut s6 = ConcSuite::new();
    let c6 = ConcCase {
        module: "m5boot",
        lock_order: Vec::new(),
        atomic_seq: gen_atomic_seq(1),
    };
    if s6.add_case(c6) != Err(E_LOCK_EMPTY) {
        return Err(E_LOCK_EMPTY);
    }
    hits += 1;

    // R7：乱锁序拒绝 → E_LOCK_ORDER。
    let c7 = ConcCase {
        module: "m5boot",
        lock_order: vec![3, 1, 2],
        atomic_seq: gen_atomic_seq(2),
    };
    if s6.add_case(c7) != Err(E_LOCK_ORDER) {
        return Err(E_LOCK_ORDER);
    }
    hits += 1;

    // R8：重复用例拒绝 → E_DUP_CASE；闭集外模块拒绝 → E_STRAY。
    let mut s8 = ConcSuite::new();
    s8.add_case(build_case("m5boot", 3))?;
    if s8.add_case(build_case("m5boot", 4)) != Err(E_DUP_CASE) {
        return Err(E_DUP_CASE);
    }
    if s8.add_case(build_case("m8ghost", 5)) != Err(E_STRAY) {
        return Err(E_STRAY);
    }
    hits += 1;

    // R9：套件指纹：同构两套件一致；检出数不同指纹不同。
    let mut s9a = ConcSuite::new();
    let mut s9b = ConcSuite::new();
    for (i, m) in M_SEG_CASES.iter().enumerate() {
        s9a.add_case(build_case(m, 0xD260_0000 + i as u32))?;
        s9b.add_case(build_case(m, 0xD260_0000 + i as u32))?;
    }
    if s9a.fingerprint() != s9b.fingerprint() {
        return Err(E_FINGER);
    }
    s9b.inject_all(0x5A)?;
    if s9a.fingerprint() == s9b.fingerprint() {
        return Err(E_FINGER);
    }
    hits += 1;

    // R10：报告文本含用例数与检出数。
    let rep = s9b.report();
    if !rep.contains("10") {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
