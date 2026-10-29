//! UNX-F48551 —— E28 三类窗口恢复回归哨兵
//! 判据原文：UNX-F48551-J1 断电重启后三类窗口（vxwm 原生 / Wine /
//! Xwayland）必须立即可输入：每类窗口恢复路径以回归哨兵核收，
//! 恢复耗时 ≤ RECOVER_BUDGET_MS、输入就绪位必须置位；
//! 三类哨兵互不串账（分类闭集）、回归账守恒（三类各恰一哨兵通过）、
//! 超时必检出、指纹与呈现行可复核。
//! 判据落地（drill 场景）：R1 三类闭集；R2 原生窗哨兵；
//! R3 Wine 窗哨兵；R4 Xwayland 窗哨兵；R5 预算边界；
//! R6 超时检出；R7 三类账守恒；R8 fuzz 恢复流零错态；
//! R9 指纹稳定；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xF3C0;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_KIND: u16 = SEG_BASE + 1; // 窗口类别越出闭集
const E_READY: u16 = SEG_BASE + 2; // 就绪位异常
const E_SLOW: u16 = SEG_BASE + 3; // 恢复超时未检出
const E_BOUND: u16 = SEG_BASE + 4; // 预算边界判定错误
const E_CONSERV: u16 = SEG_BASE + 5; // 哨兵账不守恒
const E_MIX: u16 = SEG_BASE + 6; // 三类串账
const E_FUZZ: u16 = SEG_BASE + 7; // fuzz 恢复流错态
const E_FP: u16 = SEG_BASE + 8; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 9; // 呈现行缺段
const E_NORECOVER: u16 = SEG_BASE + 10; // 恢复路径未闭合

/// 恢复预算（毫秒）：立即可输入的上限。
pub const RECOVER_BUDGET_MS: u32 = 1500;

/// 窗口类别闭集：1 vxwm 原生 / 2 Wine / 3 Xwayland。
pub const W_NATIVE: u8 = 1;
pub const W_WINE: u8 = 2;
pub const W_XWAY: u8 = 3;
pub const KIND_NAMES: [&str; 3] = ["vxwm-native", "wine", "xwayland"];

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

/// 窗口类别闭集判定。
pub fn known_kind(k: u8) -> bool {
    k == W_NATIVE || k == W_WINE || k == W_XWAY
}

/// 一笔恢复哨兵样本：类别 + 恢复耗时。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RecoverSample {
    pub kind: u8,
    pub ms: u32,
}

/// 哨兵账：三类各自的通过/超时计数与最近耗时。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KindSentinel {
    pub kind: u8,
    pub passes: u32,
    pub timeouts: u32,
    pub last_ms: u32,
    pub ready: bool,
}

/// E28 回归哨兵面：三类窗口恢复核收。
pub struct SentinelBank {
    pub sentinels: Vec<KindSentinel>,
}

impl SentinelBank {
    pub fn new() -> SentinelBank {
        SentinelBank { sentinels: vec![] }
    }

    fn idx(&self, kind: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.sentinels.len() {
            if self.sentinels[i].kind == kind {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 采一笔恢复样本：类别闭集、就绪位判定、超时记账。
    /// 返回 Ok(true) 表示达标（≤ 预算且就绪），Ok(false) 表示超时。
    pub fn sample(&mut self, s: RecoverSample) -> Result<bool, u16> {
        if !known_kind(s.kind) {
            return Err(E_KIND);
        }
        if self.idx(s.kind).is_none() {
            self.sentinels.push(KindSentinel {
                kind: s.kind,
                passes: 0,
                timeouts: 0,
                last_ms: 0,
                ready: false,
            });
        }
        let idx = self.idx(s.kind);
        let i = match idx {
            Some(i) => i,
            None => return Err(E_KIND),
        };
        self.sentinels[i].last_ms = s.ms;
        if s.ms <= RECOVER_BUDGET_MS {
            self.sentinels[i].passes += 1;
            self.sentinels[i].ready = true;
            Ok(true)
        } else {
            self.sentinels[i].timeouts += 1;
            self.sentinels[i].ready = false;
            Ok(false)
        }
    }

    /// 单类就绪位查询。
    pub fn ready_of(&self, kind: u8) -> bool {
        let mut i = 0usize;
        while i < self.sentinels.len() {
            if self.sentinels[i].kind == kind {
                return self.sentinels[i].ready;
            }
            i += 1;
        }
        false
    }

    /// 三类账守恒：闭集三类各有哨兵且 pass+timeout == 采样数。
    pub fn conserved(&self, per_kind: &[(u8, u32)]) -> bool {
        if self.sentinels.len() != 3 {
            return false;
        }
        let mut i = 0usize;
        while i < per_kind.len() {
            let mut hit = false;
            let mut j = 0usize;
            while j < self.sentinels.len() {
                if self.sentinels[j].kind == per_kind[i].0 {
                    let s = self.sentinels[j].passes + self.sentinels[j].timeouts;
                    if s != per_kind[i].1 {
                        return false;
                    }
                    hit = true;
                }
                j += 1;
            }
            if !hit {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 三类闭集：1..=3 合法、0/4/255 非法；类名非空互异。
    if !known_kind(W_NATIVE) || !known_kind(W_WINE) || !known_kind(W_XWAY) {
        return Err(E_KIND);
    }
    if known_kind(0) || known_kind(4) || known_kind(255) || KIND_NAMES[0] == KIND_NAMES[2] {
        return Err(E_KIND);
    }
    hits += 1;

    // R2 原生窗哨兵：断电后恢复 800ms 达标、就绪置位。
    let mut bk = SentinelBank::new();
    let ok = bk.sample(RecoverSample { kind: W_NATIVE, ms: 800 })?;
    if !ok || !bk.ready_of(W_NATIVE) {
        return Err(E_READY);
    }
    hits += 1;

    // R3 Wine 窗哨兵：恢复 1200ms 达标、就绪置位。
    let ok = bk.sample(RecoverSample { kind: W_WINE, ms: 1200 })?;
    if !ok || !bk.ready_of(W_WINE) {
        return Err(E_READY);
    }
    hits += 1;

    // R4 Xwayland 窗哨兵：恢复 1400ms 达标、就绪置位。
    let ok = bk.sample(RecoverSample { kind: W_XWAY, ms: 1400 })?;
    if !ok || !bk.ready_of(W_XWAY) {
        return Err(E_READY);
    }
    hits += 1;

    // R5 预算边界：恰 1500 达标、1501 超时。
    let mut bk2 = SentinelBank::new();
    let ok = bk2.sample(RecoverSample {
        kind: W_NATIVE,
        ms: RECOVER_BUDGET_MS,
    })?;
    if !ok {
        return Err(E_BOUND);
    }
    let ok = bk2.sample(RecoverSample {
        kind: W_NATIVE,
        ms: RECOVER_BUDGET_MS + 1,
    })?;
    if ok || bk2.ready_of(W_NATIVE) {
        return Err(E_BOUND);
    }
    hits += 1;

    // R6 超时检出：三类各一笔超时样本全部记账。
    let mut bk3 = SentinelBank::new();
    let kinds = [W_NATIVE, W_WINE, W_XWAY];
    let mut i = 0usize;
    while i < kinds.len() {
        let ok = bk3.sample(RecoverSample {
            kind: kinds[i],
            ms: RECOVER_BUDGET_MS + 100,
        })?;
        if ok || bk3.ready_of(kinds[i]) {
            return Err(E_SLOW);
        }
        i += 1;
    }
    hits += 1;

    // R7 三类账守恒：三类各 1 笔（bk3 全超时）、互不串账。
    let per = [(W_NATIVE, 1u32), (W_WINE, 1u32), (W_XWAY, 1u32)];
    if !bk3.conserved(&per) || !bk.conserved(&per) {
        return Err(E_CONSERV);
    }
    // 串账反证：单类账内他类就绪位恒假。
    if bk3.ready_of(W_WINE) && bk3.ready_of(W_XWAY) && bk3.ready_of(W_NATIVE) {
        return Err(E_MIX);
    }
    let mut bk3b = SentinelBank::new();
    bk3b.sample(RecoverSample { kind: W_NATIVE, ms: 100 })?;
    if bk3b.ready_of(W_WINE) || bk3b.ready_of(W_XWAY) {
        return Err(E_MIX);
    }
    // 恢复路径未闭合：未采样的类别就绪位恒假（零幽灵就绪）。
    if bk3b.ready_of(W_XWAY) {
        return Err(E_NORECOVER);
    }
    hits += 1;

    // R8 fuzz 恢复流零错态：LCG 驱动 96 笔混合样本（三类 × 达标/超时），
    // 每笔按判定记账，末态守恒、达标样本就绪位为真。
    let mut bk4 = SentinelBank::new();
    let mut lcg = Lcg::new(0x48551);
    let per = [(W_NATIVE, 32u32), (W_WINE, 32u32), (W_XWAY, 32u32)];
    let mut t = 0u32;
    while t < 96 {
        let kind = (1 + lcg.below(3)) as u8;
        let ms = lcg.below(3001);
        let ok = bk4.sample(RecoverSample { kind, ms })?;
        if ok != (ms <= RECOVER_BUDGET_MS) {
            return Err(E_FUZZ);
        }
        t += 1;
    }
    if !bk4.conserved(&per) {
        return Err(E_FUZZ);
    }
    hits += 1;

    // R9 指纹稳定：同账两次指纹一致且非零。
    let f1 = bank_fp(&bk4);
    let f2 = bank_fp(&bk4);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行齐全：nat=/wine=/xway= 三段前缀在位。
    let line = bank_line(&bk);
    if !has_kw(line.as_bytes(), b"nat=")
        || !has_kw(line.as_bytes(), b"wine=")
        || !has_kw(line.as_bytes(), b"xway=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

/// 账面指纹：三类哨兵字段串联。
pub fn bank_fp(bk: &SentinelBank) -> u64 {
    let mut buf: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < bk.sentinels.len() {
        buf.push(bk.sentinels[i].kind);
        buf.extend_from_slice(&bk.sentinels[i].passes.to_le_bytes());
        buf.extend_from_slice(&bk.sentinels[i].timeouts.to_le_bytes());
        buf.extend_from_slice(&bk.sentinels[i].last_ms.to_le_bytes());
        buf.push(bk.sentinels[i].ready as u8);
        i += 1;
    }
    fnv1a64(&buf)
}

/// 呈现行：nat=<原生通过> wine=<Wine 通过> xway=<Xwayland 通过>。
pub fn bank_line(bk: &SentinelBank) -> String {
    let mut s = String::new();
    s.push_str("nat=");
    s.push_str(&pass_of(bk, W_NATIVE).to_string());
    s.push_str(" wine=");
    s.push_str(&pass_of(bk, W_WINE).to_string());
    s.push_str(" xway=");
    s.push_str(&pass_of(bk, W_XWAY).to_string());
    s
}

/// 单类通过数查询。
pub fn pass_of(bk: &SentinelBank, kind: u8) -> u32 {
    let mut i = 0usize;
    while i < bk.sentinels.len() {
        if bk.sentinels[i].kind == kind {
            return bk.sentinels[i].passes;
        }
        i += 1;
    }
    0
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
