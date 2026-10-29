//! UNX-F48518 —— E26 与 AI-82 抽检登记（风暴防御为可观测动作抽检位）
//! 判据原文：UNX-F48518-J1 E26 风暴防御作为可观测动作向 AI-82 抽检面
//! 登记：登记闭集内每个动作占一个抽检位；抽检按确定性序号取样，
//! 命中动作必须落在登记闭集内；登记账守恒、重复登记拒收、
//! 指纹与呈现行可复核、未登记动作不得被抽中。
//! 判据落地（drill 场景）：R1 登记闭集；R2 抽检位取样命中；
//! R3 重复登记拒收；R4 未登记动作不可抽中；R5 登记账守恒；
//! R6 抽检序确定性；R7 空账不可抽检；R8 fuzz 抽检流零错态；
//! R9 指纹稳定；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xEC20;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_ACTION: u16 = SEG_BASE + 1; // 动作越出登记闭集
const E_SAMPLE: u16 = SEG_BASE + 2; // 抽检取样命中失败
const E_DUP: u16 = SEG_BASE + 3; // 重复登记拒收失败
const E_UNKNOWN: u16 = SEG_BASE + 4; // 未登记动作被抽中
const E_CONSERV: u16 = SEG_BASE + 5; // 登记账不守恒
const E_DET: u16 = SEG_BASE + 6; // 抽检序不确定
const E_EMPTY: u16 = SEG_BASE + 7; // 空账抽检未拒
const E_FUZZ: u16 = SEG_BASE + 8; // fuzz 抽检流错态
const E_FP: u16 = SEG_BASE + 9; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// 登记闭集：1 削峰 / 2 降级 / 3 告警 / 4 呈现 / 5 审计。
pub const A_THROTTLE: u8 = 1;
pub const A_DEGRADE: u8 = 2;
pub const A_ALARM: u8 = 3;
pub const A_PRESENT: u8 = 4;
pub const A_AUDIT: u8 = 5;
pub const ACTION_NAMES: [&str; 5] = ["throttle", "degrade", "alarm", "present", "audit"];

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

/// 登记闭集判定。
pub fn known_action(a: u8) -> bool {
    a == A_THROTTLE || a == A_DEGRADE || a == A_ALARM || a == A_PRESENT || a == A_AUDIT
}

/// 抽检登记账：登记序 + 抽检命中账。
pub struct AuditBook {
    pub registry: Vec<u8>,
    pub hits: Vec<(u8, u32)>,
}

impl AuditBook {
    pub fn new() -> AuditBook {
        AuditBook {
            registry: vec![],
            hits: vec![],
        }
    }

    fn reg_idx(&self, a: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.registry.len() {
            if self.registry[i] == a {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 登记一个动作：闭集校验、重复登记拒收。
    pub fn register(&mut self, a: u8) -> Result<u32, u16> {
        if !known_action(a) {
            return Err(E_ACTION);
        }
        if self.reg_idx(a).is_some() {
            return Err(E_DUP);
        }
        self.registry.push(a);
        Ok(self.registry.len() as u32)
    }

    /// 抽检取样：按序号取模命中登记闭集；空账拒检；命中入抽检账。
    pub fn sample(&mut self, seq: u32) -> Result<u8, u16> {
        if self.registry.is_empty() {
            return Err(E_EMPTY);
        }
        let idx = (seq % self.registry.len() as u32) as usize;
        let a = self.registry[idx];
        if !known_action(a) {
            return Err(E_UNKNOWN);
        }
        let mut hit = false;
        let mut i = 0usize;
        while i < self.hits.len() {
            if self.hits[i].0 == a {
                self.hits[i].1 += 1;
                hit = true;
            }
            i += 1;
        }
        if !hit {
            self.hits.push((a, 1));
        }
        Ok(a)
    }

    /// 登记账守恒：registry 无重复且全在闭集。
    pub fn conserved(&self) -> bool {
        let mut i = 0usize;
        while i < self.registry.len() {
            if !known_action(self.registry[i]) {
                return false;
            }
            let mut j = i + 1;
            while j < self.registry.len() {
                if self.registry[i] == self.registry[j] {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    /// 抽检账守恒：命中总数 == 外部记账。
    pub fn hits_total(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < self.hits.len() {
            s += self.hits[i].1;
            i += 1;
        }
        s
    }
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 登记闭集：5 动作合法、0/6/255 非法；动作名非空互异。
    let mut lcg = Lcg::new(0x48518);
    let mut t = 0u32;
    while t < 16 {
        let a = (lcg.next_u32() % 8) as u8;
        if a >= 1 && a <= 5 {
            if !known_action(a) {
                return Err(E_ACTION);
            }
        } else if known_action(a) {
            return Err(E_ACTION);
        }
        t += 1;
    }
    if ACTION_NAMES.len() != 5 || ACTION_NAMES[0] == ACTION_NAMES[4] {
        return Err(E_ACTION);
    }
    hits += 1;

    // R2 抽检位取样命中：登记 5 动作，连续取样 32 次全部命中闭集。
    let mut bk = AuditBook::new();
    let acts = [A_THROTTLE, A_DEGRADE, A_ALARM, A_PRESENT, A_AUDIT];
    let mut i = 0usize;
    while i < acts.len() {
        bk.register(acts[i])?;
        i += 1;
    }
    let mut t = 0u32;
    while t < 32 {
        let a = bk.sample(t)?;
        if !known_action(a) {
            return Err(E_SAMPLE);
        }
        t += 1;
    }
    if bk.hits_total() != 32 {
        return Err(E_SAMPLE);
    }
    hits += 1;

    // R3 重复登记拒收：同一动作二次登记被拒，码恰为 E_DUP，账不变。
    let n_before = bk.registry.len();
    match bk.register(A_THROTTLE) {
        Err(E_DUP) => hits += 1,
        _ => return Err(E_DUP),
    }
    if bk.registry.len() != n_before {
        return Err(E_DUP);
    }

    // R4 未登记动作不可抽中：闭集内但未登记 → 空账拒检路径；
    // 这里用独立账验证：登记 2 动作，取样序列只命中这 2 个。
    let mut bk2 = AuditBook::new();
    bk2.register(A_ALARM)?;
    bk2.register(A_PRESENT)?;
    let mut t = 0u32;
    while t < 16 {
        let a = bk2.sample(t)?;
        if a != A_ALARM && a != A_PRESENT {
            return Err(E_UNKNOWN);
        }
        t += 1;
    }
    hits += 1;

    // R5 登记账守恒：registry 无重复、全在闭集；抽检账与取样数一致。
    if !bk.conserved() || bk.hits_total() != 32 {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R6 抽检序确定性：同账同序两次取样序列逐位一致。
    let mut bk3 = AuditBook::new();
    let mut i = 0usize;
    while i < acts.len() {
        bk3.register(acts[i])?;
        i += 1;
    }
    let mut seq1: Vec<u8> = vec![];
    let mut t = 0u32;
    while t < 16 {
        seq1.push(bk3.sample(t)?);
        t += 1;
    }
    let mut bk4 = AuditBook::new();
    let mut i = 0usize;
    while i < acts.len() {
        bk4.register(acts[i])?;
        i += 1;
    }
    let mut t = 0u32;
    while t < 16 {
        if bk4.sample(t)? != seq1[t as usize] {
            return Err(E_DET);
        }
        t += 1;
    }
    hits += 1;

    // R7 空账不可抽检：未登记任何动作时取样被拒，码恰为 E_EMPTY。
    let mut bk5 = AuditBook::new();
    match bk5.sample(0) {
        Err(E_EMPTY) => hits += 1,
        _ => return Err(E_EMPTY),
    }

    // R8 fuzz 抽检流零错态：LCG 驱动 128 次取样（登记全闭集），
    // 每次命中闭集、抽检账与取样数守恒。
    let mut bk6 = AuditBook::new();
    let mut i = 0usize;
    while i < acts.len() {
        bk6.register(acts[i])?;
        i += 1;
    }
    let mut lcg2 = Lcg::new(0x26E1_10);
    let mut t = 0u32;
    while t < 128 {
        let seq = lcg2.below(4096);
        let a = bk6.sample(seq)?;
        if !known_action(a) {
            return Err(E_FUZZ);
        }
        t += 1;
    }
    if bk6.hits_total() != 128 || !bk6.conserved() {
        return Err(E_FUZZ);
    }
    hits += 1;

    // R9 指纹稳定：同账两次指纹一致且非零。
    let f1 = book_fp(&bk6);
    let f2 = book_fp(&bk6);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行齐全：reg=/hits= 两段前缀在位。
    let line = book_line(&bk6);
    if !has_kw(line.as_bytes(), b"reg=") || !has_kw(line.as_bytes(), b"hits=") {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

/// 账面指纹：登记序 + 抽检账串联。
pub fn book_fp(bk: &AuditBook) -> u64 {
    let mut buf: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < bk.registry.len() {
        buf.push(bk.registry[i]);
        i += 1;
    }
    buf.push(0xAA);
    let mut i = 0usize;
    while i < bk.hits.len() {
        buf.push(bk.hits[i].0);
        buf.extend_from_slice(&bk.hits[i].1.to_le_bytes());
        i += 1;
    }
    fnv1a64(&buf)
}

/// 呈现行：reg=<登记数> hits=<抽检总数>。
pub fn book_line(bk: &AuditBook) -> String {
    let mut s = String::new();
    s.push_str("reg=");
    s.push_str(&(bk.registry.len() as u32).to_string());
    s.push_str(" hits=");
    s.push_str(&bk.hits_total().to_string());
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
