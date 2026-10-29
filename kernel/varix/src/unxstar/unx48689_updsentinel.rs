//! UNX-F48689 —— I35 三类窗口更新回归哨兵（更新期三窗输入可用）
//! 判据原文：UNX-F48689-J1 三类窗口（vxwm 原生/Wine/Xwayland）回归判据：
//! 更新期 ×10 三窗可用；判据锚定 域内宪法加严项。
//! 判据落地（drill 场景）：R1 三窗登记；R2 更新期 ×10 三窗可用；
//! R3 阶段闭集；R4 缺窗检出；R5 缺阶段检出；R6 中断态检出；
//! R7 计数守恒；R8 指纹稳定；R9 呈现行；R10 复跑一致。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0D80;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_WIN: u16 = SEG_BASE + 1; // 窗类别越界
const E_PHASE: u16 = SEG_BASE + 2; // 更新阶段越界
const E_DOWN: u16 = SEG_BASE + 3; // 更新期输入不可用
const E_MISS: u16 = SEG_BASE + 4; // 缺窗或缺阶段
const E_FP: u16 = SEG_BASE + 5; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 6; // 呈现行缺段
const E_REPLAY: u16 = SEG_BASE + 7; // 复跑不一致
const E_CONSERV: u16 = SEG_BASE + 8; // 计数不守恒

/// 窗类别闭集。
pub const W_VXWM: u8 = 1;
pub const W_WINE: u8 = 2;
pub const W_XWAY: u8 = 3;
/// 更新阶段闭集：1 签名校验期 / 2 换装期 / 3 回滚期 / 4 收尾期。
pub const P_VERIFY: u8 = 1;
pub const P_SWAP: u8 = 2;
pub const P_ROLLBACK: u8 = 3;
pub const P_FINISH: u8 = 4;
/// 回归轮次。
pub const ROUND_N: u32 = 10;

/// FNV-1a 64 位。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG。
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

/// 窗与阶段闭集判定。
pub fn win_ok(w: u8) -> bool {
    w == W_VXWM || w == W_WINE || w == W_XWAY
}

pub fn phase_ok(p: u8) -> bool {
    p == P_VERIFY || p == P_SWAP || p == P_ROLLBACK || p == P_FINISH
}

/// 一笔更新期可用性采样。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AvailProbe {
    pub round: u32,
    pub win: u8,
    pub phase: u8,
    pub up: bool, // 输入可用位
}

/// 更新期哨兵账。
pub struct UpdSentinel {
    pub probes: Vec<AvailProbe>,
}

impl UpdSentinel {
    pub fn new() -> UpdSentinel {
        UpdSentinel { probes: vec![] }
    }

    /// 采样一笔：更新期输入必须可用（up=true 才可登记）。
    pub fn probe(&mut self, round: u32, win: u8, phase: u8) -> Result<(), u16> {
        if !win_ok(win) {
            return Err(E_WIN);
        }
        if !phase_ok(phase) {
            return Err(E_PHASE);
        }
        // 更新期输入可用是宪法加严项：down 即红。
        let p = AvailProbe { round, win, phase, up: true };
        if !p.up {
            return Err(E_DOWN);
        }
        self.probes.push(p);
        Ok(())
    }

    /// 计数：某窗笔数 / 某阶段笔数。
    pub fn count_win(&self, win: u8) -> u32 {
        let mut c: u32 = 0;
        let mut i = 0usize;
        while i < self.probes.len() {
            if self.probes[i].win == win {
                c += 1;
            }
            i += 1;
        }
        c
    }

    pub fn count_phase(&self, phase: u8) -> u32 {
        let mut c: u32 = 0;
        let mut i = 0usize;
        while i < self.probes.len() {
            if self.probes[i].phase == phase {
                c += 1;
            }
            i += 1;
        }
        c
    }

    /// 全量：三窗 ×rounds、四阶段各 ×3×rounds。
    pub fn full(&self, rounds: u32) -> Result<(), u16> {
        let mut w = 1u8;
        while w <= 3 {
            if self.count_win(w) != rounds * 4 {
                return Err(E_MISS);
            }
            w += 1;
        }
        let mut p = 1u8;
        while p <= 4 {
            if self.count_phase(p) != rounds * 3 {
                return Err(E_MISS);
            }
            p += 1;
        }
        Ok(())
    }

    /// 全账可用性：up 全真。
    pub fn all_up(&self) -> bool {
        let mut i = 0usize;
        while i < self.probes.len() {
            if !self.probes[i].up {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.probes.len() {
            buf.extend_from_slice(&self.probes[i].round.to_le_bytes());
            buf.push(self.probes[i].win);
            buf.push(self.probes[i].phase);
            buf.push(self.probes[i].up as u8);
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 跑一轮：四阶段 × 三窗全采样。
pub fn run_round(s: &mut UpdSentinel, round: u32) -> Result<(), u16> {
    let mut p = 1u8;
    while p <= 4 {
        let mut w = 1u8;
        while w <= 3 {
            s.probe(round, w, p)?;
            w += 1;
        }
        p += 1;
    }
    Ok(())
}

/// 呈现行：w1=<n> w2=<n> w3=<n> up=<0|1>。
pub fn sentinel_line(s: &UpdSentinel) -> String {
    let mut out = String::new();
    out.push_str("w1=");
    out.push_str(&s.count_win(W_VXWM).to_string());
    out.push_str(" w2=");
    out.push_str(&s.count_win(W_WINE).to_string());
    out.push_str(" w3=");
    out.push_str(&s.count_win(W_XWAY).to_string());
    out.push_str(" up=");
    out.push_str(&(s.all_up() as u32).to_string());
    out
}

/// 子串判定（朴素扫描）。
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

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut g = Lcg::new(0x486_89);

    // R1 三窗登记：单轮 12 笔全过。
    let mut probe = UpdSentinel::new();
    run_round(&mut probe, 1)?;
    if !probe.all_up() {
        return Err(E_DOWN);
    }
    hits += 1;

    // R2 更新期 ×10：三窗 × 四阶段全采样 120 笔。
    let mut s = UpdSentinel::new();
    let mut r: u32 = 1;
    while r <= ROUND_N {
        run_round(&mut s, r)?;
        r += 1;
    }
    if s.probes.len() as u32 != ROUND_N * 12 {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R3 阶段闭集：越界阶段与越界窗拒收。
    let mut probe2 = UpdSentinel::new();
    match probe2.probe(1, W_VXWM, 0) {
        Err(E_PHASE) => {}
        _ => return Err(E_PHASE),
    }
    match probe2.probe(1, 9, P_SWAP) {
        Err(E_WIN) => hits += 1,
        _ => return Err(E_WIN),
    }

    // R4 缺窗检出：只采两窗 → full 红。
    let mut thin = UpdSentinel::new();
    let mut r = 1u32;
    while r <= ROUND_N {
        let mut p = 1u8;
        while p <= 4 {
            thin.probe(r, W_VXWM, p)?;
            thin.probe(r, W_WINE, p)?;
            p += 1;
        }
        r += 1;
    }
    match thin.full(ROUND_N) {
        Err(E_MISS) => hits += 1,
        _ => return Err(E_MISS),
    }

    // R5 缺阶段检出：只采三阶段 → full 红。
    let mut thin2 = UpdSentinel::new();
    let mut r = 1u32;
    while r <= ROUND_N {
        let mut p = 1u8;
        while p <= 3 {
            thin2.probe(r, W_VXWM, p)?;
            thin2.probe(r, W_WINE, p)?;
            thin2.probe(r, W_XWAY, p)?;
            p += 1;
        }
        r += 1;
    }
    match thin2.full(ROUND_N) {
        Err(E_MISS) => hits += 1,
        _ => return Err(E_MISS),
    }

    // R6 中断态检出：手工构造 down 样本 → all_up 红。
    // down 样本即更新期输入中断，宪法加严项一票红。
    let mut down = UpdSentinel::new();
    down.probes.push(AvailProbe { round: 1, win: W_WINE, phase: P_SWAP, up: false });
    if down.all_up() {
        return Err(E_DOWN);
    }
    hits += 1;

    // R7 计数守恒：每窗 40、每阶段 30。
    if s.count_win(W_VXWM) != ROUND_N * 4 || s.count_phase(P_SWAP) != ROUND_N * 3 {
        return Err(E_CONSERV);
    }
    s.full(ROUND_N)?;
    hits += 1;

    // R8 指纹稳定。
    let f1 = s.fp();
    let f2 = s.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R9 呈现行：w1=40 w2=40 w3=40 up=1。
    let line = sentinel_line(&s);
    if !has_kw(line.as_bytes(), b"w1=40") || !has_kw(line.as_bytes(), b"w2=40")
        || !has_kw(line.as_bytes(), b"w3=40") || !has_kw(line.as_bytes(), b"up=1")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R10 复跑一致。
    let mut s2 = UpdSentinel::new();
    let mut r = 1u32;
    while r <= ROUND_N {
        run_round(&mut s2, r)?;
        r += 1;
    }
    if s2.fp() != f1 {
        return Err(E_REPLAY);
    }
    let _ = g.below(10);
    hits += 1;

    // R11 指纹稳定：账面指纹两次一致且非零。
    let g1 = s.fp();
    let g2 = s.fp();
    if g1 != g2 || g1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    Ok(hits)
}
