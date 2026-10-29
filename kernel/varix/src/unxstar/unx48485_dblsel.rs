//! UNX-F48485 —— E·连点候选选择防重（正：双击不触发双份上屏）
//! 判据原文：UNX-F48485-J1 候选上屏通道对同一候选在双击窗口内的二次提交
//! 必须去重：双击只上屏一份；窗口外再次提交才允许新一笔；
//! 候选号闭集、上屏账守恒、时戳单调、指纹可复核。
//! 判据落地（drill 场景）：R1 候选闭集；R2 双击去重（窗口内二笔拒收）；
//! R3 窗口外放行；R4 上屏账守恒（账笔数 == 通过笔数）；
//! R5 拒收零副作用；R6 时戳单调流放行/乱序流检出；
//! R7 候选互异去重；R8 指纹稳定；R9 呈现行齐全；R10 未知候选拒收。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xE6C0;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_CAND: u16 = SEG_BASE + 1; // 候选号越出闭集
const E_DUPWIN: u16 = SEG_BASE + 2; // 双击窗口内重复提交被拒
const E_CONSERV: u16 = SEG_BASE + 3; // 上屏账不守恒
const E_SIDE: u16 = SEG_BASE + 4; // 拒收产生副作用
const E_ORDER: u16 = SEG_BASE + 5; // 时戳非单调
const E_FP: u16 = SEG_BASE + 6; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 7; // 呈现行缺段

/// 双击去重窗口（毫秒）。
pub const DUP_WIN_MS: u32 = 250;
/// 候选号闭集：1..=9。
pub const CAND_LO: u8 = 1;
pub const CAND_HI: u8 = 9;

/// 一笔候选提交。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pick {
    pub cand: u8,
    pub at: u32,
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

/// 上屏账一笔：候选 + 通过时刻。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Commit {
    pub cand: u8,
    pub at: u32,
}

/// 上屏通道：双击窗口内同候选去重。
pub struct CommitGate {
    pub ledger: Vec<Commit>,
    pub last: Option<Pick>,
}

impl CommitGate {
    pub fn new() -> CommitGate {
        CommitGate {
            ledger: vec![],
            last: None,
        }
    }

    /// 候选闭集判定。
    pub fn known_cand(c: u8) -> bool {
        c >= CAND_LO && c <= CAND_HI
    }

    /// 提交一笔：同候选在 DUP_WIN_MS 内重复 → 拒收且零副作用。
    pub fn submit(&mut self, p: Pick) -> Result<(), u16> {
        if !CommitGate::known_cand(p.cand) {
            return Err(E_CAND);
        }
        if let Some(last) = self.last {
            if last.cand == p.cand && p.at.saturating_sub(last.at) < DUP_WIN_MS {
                // 拒收路径：账与 last 均不得改动（零副作用）。
                return Err(E_DUPWIN);
            }
        }
        self.ledger.push(Commit { cand: p.cand, at: p.at });
        self.last = Some(p);
        Ok(())
    }

    /// 账守恒：账内每笔候选均通过闭集，且 last 与账尾一致。
    pub fn conserved(&self) -> bool {
        let mut i = 0usize;
        while i < self.ledger.len() {
            if !CommitGate::known_cand(self.ledger[i].cand) {
                return false;
            }
            i += 1;
        }
        match self.last {
            Some(p) => match self.ledger.last() {
                Some(c) => *c == Commit { cand: p.cand, at: p.at },
                None => false,
            },
            None => self.ledger.is_empty(),
        }
    }

    /// 账内候选总数。
    pub fn count(&self) -> u32 {
        self.ledger.len() as u32
    }
}

/// 时戳单调判定。
pub fn monotonic(ps: &[Pick]) -> bool {
    let mut i = 1usize;
    while i < ps.len() {
        if ps[i - 1].at > ps[i].at {
            return false;
        }
        i += 1;
    }
    true
}

/// 上屏指纹：账逐笔串联。
pub fn gate_fp(g: &CommitGate) -> u64 {
    let mut buf: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < g.ledger.len() {
        buf.push(g.ledger[i].cand);
        buf.extend_from_slice(&g.ledger[i].at.to_le_bytes());
        i += 1;
    }
    fnv1a64(&buf)
}

/// 上屏行（呈现面）：n=<笔数> last=<候选@时戳>。
pub fn gate_line(g: &CommitGate) -> String {
    let mut s = String::new();
    s.push_str("n=");
    s.push_str(&g.count().to_string());
    s.push_str(" last=");
    match g.last {
        Some(p) => {
            s.push_str(&(p.cand as u32).to_string());
            s.push_str("@");
            s.push_str(&p.at.to_string());
        }
        None => s.push_str("none"),
    }
    s
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 候选闭集：1..=9 全知，0/10 越集。
    let mut c = CAND_LO;
    while c <= CAND_HI {
        if !CommitGate::known_cand(c) {
            return Err(E_CAND);
        }
        c += 1;
    }
    if CommitGate::known_cand(0) || CommitGate::known_cand(10) {
        return Err(E_CAND);
    }
    hits += 1;

    // R2 双击去重：同候选 250ms 内二笔必拒且诊断码恰为 E_DUPWIN。
    let mut g = CommitGate::new();
    g.submit(Pick { cand: 3, at: 1000 })?;
    let before = gate_fp(&g);
    match g.submit(Pick { cand: 3, at: 1000 + DUP_WIN_MS - 1 }) {
        Err(E_DUPWIN) => hits += 1,
        _ => return Err(E_DUPWIN),
    }

    // R5 拒收零副作用：拒后指纹与账笔数不变。
    if gate_fp(&g) != before || g.count() != 1 {
        return Err(E_SIDE);
    }
    hits += 1;

    // R3 窗口外放行：超窗一笔（间隔恰 = 窗与 = 窗+1）都通过。
    g.submit(Pick { cand: 3, at: 1000 + DUP_WIN_MS })?;
    g.submit(Pick { cand: 3, at: 1000 + DUP_WIN_MS * 2 + 1 })?;
    if g.count() != 3 {
        return Err(E_CONSERV);
    }
    // 窗外换候选立即放行。
    g.submit(Pick { cand: 5, at: 1600 })?;
    if g.count() != 4 {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R4 上屏账守恒：账笔数 == 通过笔数，last 恰为账尾。
    if !g.conserved() {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R6 时戳单调：有序流放行、乱序流检出。
    let mut lcg = Lcg::new(0x48485);
    let mut seq: Vec<Pick> = vec![];
    let mut t = 0u32;
    while t < 8 {
        seq.push(Pick {
            cand: 1 + lcg.below(9) as u8,
            at: t * 400, // 间隔 ≥ 窗，全部可过
        });
        t += 1;
    }
    if !monotonic(&seq) {
        return Err(E_ORDER);
    }
    let mut bad = seq.clone();
    bad.insert(3, Pick { cand: 2, at: 0 });
    if monotonic(&bad) {
        return Err(E_ORDER);
    }
    hits += 1;

    // R7 候选互异去重：九候选各一笔全部入账，账内无重复对（间隔≥窗）。
    let mut g2 = CommitGate::new();
    let mut c = CAND_LO;
    let mut at = 0u32;
    while c <= CAND_HI {
        g2.submit(Pick { cand: c, at })?;
        c += 1;
        at += DUP_WIN_MS + 10;
    }
    if g2.count() != 9 {
        return Err(E_CONSERV);
    }
    let mut i = 0usize;
    while i < g2.ledger.len() {
        let mut j = i + 1;
        while j < g2.ledger.len() {
            if g2.ledger[i].cand == g2.ledger[j].cand && g2.ledger[i].at == g2.ledger[j].at {
                return Err(E_DUPWIN);
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;

    // R8 指纹稳定：同账两次指纹一致且非零。
    let f1 = gate_fp(&g2);
    let f2 = gate_fp(&g2);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R9 呈现行齐全：n=/last= 前缀在位，且空账 last=none。
    let line = gate_line(&g2);
    if !has_kw(line.as_bytes(), b"n=9") || !has_kw(line.as_bytes(), b"last=") {
        return Err(E_LINE);
    }
    let empty_line = gate_line(&CommitGate::new());
    if !has_kw(empty_line.as_bytes(), b"n=0") || !has_kw(empty_line.as_bytes(), b"last=none") {
        return Err(E_LINE);
    }
    hits += 1;

    // R10 未知候选拒收：0 与 10 恒拒且诊断码恰为 E_CAND。
    match g2.submit(Pick { cand: 0, at: 9999 }) {
        Err(E_CAND) => hits += 1,
        _ => return Err(E_CAND),
    }

    // R11 LCG 混合流守恒：32 笔合法间隔流（间隔 ≥ 双击窗）全部入账，
    // 账笔数 == 注入数，且窗内重复注入逐笔被拒后账面不变。
    let mut lcg2 = Lcg::new(0x48485);
    let mut g3 = CommitGate::new();
    let mut n = 0u32;
    let mut at = 0u32;
    while n < 32 {
        let cand = 1 + lcg2.below(9) as u8;
        g3.submit(Pick { cand, at })?;
        at += DUP_WIN_MS + 10;
        n += 1;
    }
    if g3.count() != 32 || !g3.conserved() {
        return Err(E_CONSERV);
    }
    // 紧贴窗口内的重复注入 8 笔全部压下，账面不变。
    let last = g3.ledger[g3.ledger.len() - 1];
    let mut k = 0u32;
    while k < 8 {
        match g3.submit(Pick { cand: last.cand, at: last.at + k }) {
            Err(E_DUPWIN) => {}
            _ => return Err(E_DUPWIN),
        }
        k += 1;
    }
    if g3.count() != 32 {
        return Err(E_SIDE);
    }
    hits += 1;

    Ok(hits)
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_CAND => "候选号越出 1..=9 闭集：核对候选面板编号后再提交",
        E_DUPWIN => "双击窗口内重复提交：同一候选 250ms 内只上屏一份",
        E_CONSERV => "上屏账不守恒：先对账通过笔数与账笔数再定位漏账",
        E_SIDE => "拒收产生副作用：拦截路径不得改动账面与 last 指针",
        E_ORDER => "提交时戳非单调：先修时序源再回放",
        E_FP => "上屏账指纹漂移：同账两次指纹不一致，查隐式全局态",
        E_LINE => "呈现行缺段：n=/last= 前缀缺一即呈现回归",
        _ => "未知诊断码",
    }
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
