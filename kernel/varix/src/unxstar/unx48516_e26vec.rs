//! UNX-F48516 —— E26 向量账（风暴用例集+校验和）
//! 判据原文：UNX-F48516-J1 E26 判定向量集以卡片为单位收账：
//! 每枚用例绑定模式（风暴/静默/边沿）、注入强度与期望放行数；
//! 全量跑分后期望必须逐枚命中；集合校验和（FNV）可复核且稳定；
//! 用例账守恒、未知模式拒收、呈现行齐全。
//! 判据落地（drill 场景）：R1 用例闭集；R2 削峰器逐枚命中；
//! R3 边界强度放行；R4 校验和稳定；R5 校验和区分扰动；
//! R6 用例账守恒；R7 未知模式拒收；R8 fuzz 用例流零错态；
//! R9 期望与削峰双向一致；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xEBC0;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_MODE: u16 = SEG_BASE + 1; // 用例模式越出闭集
const E_MISS: u16 = SEG_BASE + 2; // 期望放行未命中
const E_BOUND: u16 = SEG_BASE + 3; // 边界强度放行错误
const E_SUM: u16 = SEG_BASE + 4; // 校验和复核失败
const E_CONSERV: u16 = SEG_BASE + 5; // 用例账不守恒
const E_REJECT: u16 = SEG_BASE + 6; // 未知模式拒收失败
const E_FUZZ: u16 = SEG_BASE + 7; // fuzz 用例流错态
const E_BIDI: u16 = SEG_BASE + 8; // 期望与削峰双向不一致
const E_FP: u16 = SEG_BASE + 9; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// 用例模式闭集：1 风暴 / 2 静默 / 3 边沿。
pub const M_STORM: u8 = 1;
pub const M_SILENT: u8 = 2;
pub const M_EDGE: u8 = 3;
pub const MODE_NAMES: [&str; 3] = ["storm", "silent", "edge"];

/// 削峰上限（枚/秒）。
pub const ADMIT_CAP: u32 = 1000;

/// 一枚判定用例：模式 + 注入强度 + 期望放行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VecCase {
    pub mode: u8,
    pub load: u32,
    pub expect: u32,
}

/// FNV-1a 64 位（与本域其余哨兵同口径）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG（批内统一系数）。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 模式闭集判定。
pub fn known_mode(m: u8) -> bool {
    m == M_STORM || m == M_SILENT || m == M_EDGE
}

/// 削峰器：静默模式恒放 0；其余按 min(load, ADMIT_CAP)。
pub fn throttle(mode: u8, load: u32) -> u32 {
    if mode == M_SILENT {
        0
    } else if load <= ADMIT_CAP {
        load
    } else {
        ADMIT_CAP
    }
}

/// 用例合法性：模式在闭集内、期望与削峰器输出一致。
pub fn case_sane(c: &VecCase) -> bool {
    if !known_mode(c.mode) {
        return false;
    }
    throttle(c.mode, c.load) == c.expect
}

/// 校验和：全量用例字段串联 FNV。
pub fn cases_checksum(cases: &[VecCase]) -> u64 {
    let mut buf: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < cases.len() {
        buf.push(cases[i].mode);
        buf.extend_from_slice(&cases[i].load.to_le_bytes());
        buf.extend_from_slice(&cases[i].expect.to_le_bytes());
        i += 1;
    }
    fnv1a64(&buf)
}

/// 逐枚跑分：全部命中返回命中数，否则 Err(E_MISS)。
pub fn run_cases(cases: &[VecCase]) -> Result<u32, u16> {
    let mut pass = 0u32;
    let mut i = 0usize;
    while i < cases.len() {
        if !known_mode(cases[i].mode) {
            return Err(E_MODE);
        }
        let got = throttle(cases[i].mode, cases[i].load);
        if got != cases[i].expect {
            return Err(E_MISS);
        }
        pass += 1;
        i += 1;
    }
    Ok(pass)
}

/// 用例账：集合 + 已跑命中数 + 校验和快照。
pub struct VecBook {
    pub cases: Vec<VecCase>,
    pub passed: u32,
    pub checksum: u64,
}

impl VecBook {
    pub fn new() -> VecBook {
        VecBook {
            cases: vec![],
            passed: 0,
            checksum: 0,
        }
    }

    /// 入册一枚：模式闭集校验。
    pub fn add(&mut self, c: VecCase) -> Result<(), u16> {
        if !known_mode(c.mode) {
            return Err(E_MODE);
        }
        self.cases.push(c);
        Ok(())
    }

    /// 全量跑分并记账：逐枚命中、校验和快照、账守恒。
    pub fn settle(&mut self) -> Result<u32, u16> {
        let n = self.cases.len() as u32;
        let p = run_cases(&self.cases)?;
        if p != n {
            return Err(E_CONSERV);
        }
        self.passed = p;
        self.checksum = cases_checksum(&self.cases);
        Ok(p)
    }

    /// 账守恒：passed == 用例数。
    pub fn conserved(&self) -> bool {
        self.passed as usize == self.cases.len()
    }
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 用例闭集：mode 1..=3 合法、0/4/255 非法；模式名非空互异。
    if !known_mode(M_STORM) || !known_mode(M_SILENT) || !known_mode(M_EDGE) || known_mode(0) {
        return Err(E_MODE);
    }
    if known_mode(4) || known_mode(255) || MODE_NAMES[0] == MODE_NAMES[2] {
        return Err(E_MODE);
    }
    hits += 1;

    // R2 削峰器逐枚命中：固定用例集全绿。
    let mut bk = VecBook::new();
    let fixed = [
        VecCase { mode: M_STORM, load: 10_000, expect: ADMIT_CAP },
        VecCase { mode: M_SILENT, load: 5000, expect: 0 },
        VecCase { mode: M_EDGE, load: 800, expect: 800 },
        VecCase { mode: M_STORM, load: 300, expect: 300 },
    ];
    let mut i = 0usize;
    while i < fixed.len() {
        if !case_sane(&fixed[i]) {
            return Err(E_MISS);
        }
        bk.add(fixed[i])?;
        i += 1;
    }
    let passed = bk.settle()?;
    if passed != 4 || !bk.conserved() {
        return Err(E_MISS);
    }
    hits += 1;

    // R3 边界强度放行：0 全放、ADMIT_CAP 恰全放、ADMIT_CAP+1 削到上限。
    if throttle(M_STORM, 0) != 0 {
        return Err(E_BOUND);
    }
    if throttle(M_EDGE, ADMIT_CAP) != ADMIT_CAP {
        return Err(E_BOUND);
    }
    if throttle(M_STORM, ADMIT_CAP + 1) != ADMIT_CAP {
        return Err(E_BOUND);
    }
    hits += 1;

    // R4 校验和稳定：同集合两次校验和一致且非零。
    let c1 = cases_checksum(&bk.cases);
    let c2 = cases_checksum(&bk.cases);
    if c1 != c2 || c1 == 0 {
        return Err(E_SUM);
    }
    hits += 1;

    // R5 校验和区分扰动：任一字段翻转 → 校验和必变。
    let mut disturbed = bk.cases.clone();
    disturbed[0].load = disturbed[0].load ^ 1u32;
    if cases_checksum(&disturbed) == c1 {
        return Err(E_SUM);
    }
    hits += 1;

    // R6 用例账守恒：settle 后 passed == 数量、checksum 快照在位。
    if !bk.conserved() || bk.checksum == 0 {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R7 未知模式拒收：mode=7 入册被拒，码恰为 E_MODE。
    let mut bk2 = VecBook::new();
    match bk2.add(VecCase { mode: 7, load: 10, expect: 0 }) {
        Err(E_MODE) => hits += 1,
        _ => return Err(E_REJECT),
    }

    // R8 fuzz 用例流零错态：LCG 生成 128 枚自洽用例（期望由削峰器反推），
    // 全量跑分零错态、账守恒。
    let mut bk3 = VecBook::new();
    let mut lcg = Lcg::new(0x48516);
    let mut t = 0u32;
    while t < 128 {
        let mode = (1 + lcg.below(3)) as u8;
        let load = lcg.below(12_001);
        let expect = throttle(mode, load);
        bk3.add(VecCase { mode, load, expect })?;
        t += 1;
    }
    let p3 = bk3.settle()?;
    if p3 != 128 || !bk3.conserved() {
        return Err(E_FUZZ);
    }
    hits += 1;

    // R9 期望与削峰双向一致：逐枚 case_sane 全绿（含 fuzz 集）；
    // 账面指纹稳定同口径复核。
    let mut i = 0usize;
    while i < bk3.cases.len() {
        if !case_sane(&bk3.cases[i]) {
            return Err(E_BIDI);
        }
        i += 1;
    }
    let fp1 = book_fp(&bk3);
    let fp2 = book_fp(&bk3);
    if fp1 != fp2 || fp1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行齐全：n=/pass=/sum= 三段前缀在位。
    let line = book_line(&bk3);
    if !has_kw(line.as_bytes(), b"n=")
        || !has_kw(line.as_bytes(), b"pass=")
        || !has_kw(line.as_bytes(), b"sum=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

/// 账面指纹：passed + checksum 串联。
pub fn book_fp(bk: &VecBook) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&bk.passed.to_le_bytes());
    buf.extend_from_slice(&bk.checksum.to_le_bytes());
    fnv1a64(&buf)
}

/// 呈现行：n=<用例数> pass=<命中> sum=<校验和低 32 位>。
pub fn book_line(bk: &VecBook) -> String {
    let mut s = String::new();
    s.push_str("n=");
    s.push_str(&(bk.cases.len() as u32).to_string());
    s.push_str(" pass=");
    s.push_str(&bk.passed.to_string());
    s.push_str(" sum=");
    s.push_str(&(bk.checksum as u32).to_string());
    s
}

/// 子串判定（朴素扫描，统一口径）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}
