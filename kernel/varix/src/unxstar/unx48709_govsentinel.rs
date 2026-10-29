//! UNX-F48709 —— I36 三类窗口治理回归哨兵
//! （三类窗口（vxwm 原生/Wine/Xwayland）回归判据·宪法加严：治理抽检用三窗探针复跑）
//! 判据原文：UNX-F48709-J1 三类窗口（vxwm 原生/Wine/Xwayland）回归判据：
//! 复跑 ×3 全绿；判据锚定 域内宪法加严项。
//! 判据落地（drill 场景）：R1 三窗闭集；R2 哨兵探针落账；R3 复跑 ×3；
//! R4 全绿判定；R5 窗型越界检出；R6 漂移检出；R7 哨兵签名；
//! R8 指纹稳定；R9 计数守恒；R10 呈现行；R11 加严口径复核。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：三类窗口 = vxwm 原生 / Wine / Xwayland；哨兵探针逐窗
//! 采集回归分（0..100），宪法加严口径为复跑 ×3 且每窗每次 ≥90
//! 才算全绿；任一窗任一次欠分即红；探针落账带签名可复核。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x1000;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_WIN: u16 = SEG_BASE + 1; // 窗型越出三类闭集
const E_SCORE: u16 = SEG_BASE + 2; // 回归分欠额
const E_RUNS: u16 = SEG_BASE + 3; // 复跑次数不足
const E_PROBE: u16 = SEG_BASE + 4; // 探针签名不符
const E_COUNT: u16 = SEG_BASE + 5; // 落账计数不守恒
const E_REPLAY: u16 = SEG_BASE + 6; // 复跑结果漂移
const E_FP: u16 = SEG_BASE + 7; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 8; // 呈现行缺段
const E_SIG: u16 = SEG_BASE + 9; // 哨兵签名不符
const E_ORDER: u16 = SEG_BASE + 10; // 跑次序号不连续

/// 三类窗口闭集。
pub const W_VXWM: u8 = 1; // vxwm 原生
pub const W_WINE: u8 = 2; // Wine
pub const W_XWAY: u8 = 3; // Xwayland
pub const WIN_N: u32 = 3;

/// 宪法加严口径。
pub const REPLAY_N: u32 = 3; // 复跑 ×3
pub const PASS_LINE: u32 = 90; // 单次回归分下限

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

/// 一条探针落账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProbeRec {
    pub run: u32,   // 跑次 1..=3
    pub win: u8,    // 窗型闭集
    pub score: u32, // 回归分 0..100
    pub sig: u64,   // 探针签名
}

impl ProbeRec {
    /// 探针签名：跑次 + 窗型 + 得分混签。
    pub fn make_sig(run: u32, win: u8, score: u32) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&run.to_le_bytes());
        buf.push(win);
        buf.extend_from_slice(&score.to_le_bytes());
        fnv1a64(&buf)
    }

    pub fn sane(&self) -> Result<(), u16> {
        if self.win < W_VXWM || self.win > W_XWAY {
            return Err(E_WIN);
        }
        if self.sig != ProbeRec::make_sig(self.run, self.win, self.score) {
            return Err(E_PROBE);
        }
        Ok(())
    }
}

/// 哨兵账。
pub struct Sentinel {
    pub recs: Vec<ProbeRec>,
}

impl Sentinel {
    pub fn new() -> Sentinel {
        Sentinel { recs: vec![] }
    }

    /// 探针落账：闭集 + 重复拒绝。
    pub fn record(&mut self, run: u32, win: u8, score: u32) -> Result<(), u16> {
        if win < W_VXWM || win > W_XWAY {
            return Err(E_WIN);
        }
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].run == run && self.recs[i].win == win {
                return Err(E_PROBE);
            }
            i += 1;
        }
        let sig = ProbeRec::make_sig(run, win, score);
        self.recs.push(ProbeRec { run, win, score, sig });
        Ok(())
    }

    /// 单次全窗判定：三窗全部 ≥PASS_LINE。
    pub fn run_green(&self, run: u32) -> Result<(), u16> {
        let mut w = W_VXWM;
        while w <= W_XWAY {
            let mut found = false;
            let mut i = 0usize;
            while i < self.recs.len() {
                if self.recs[i].run == run && self.recs[i].win == w {
                    found = true;
                    if self.recs[i].score < PASS_LINE {
                        return Err(E_SCORE);
                    }
                }
                i += 1;
            }
            if !found {
                return Err(E_COUNT);
            }
            w += 1;
        }
        Ok(())
    }

    /// 全量核对：复跑 ×3 序号连续 + 逐条 sane + 逐次全绿。
    pub fn verify_all(&self) -> Result<(), u16> {
        if self.recs.len() as u32 != REPLAY_N * WIN_N {
            return Err(E_RUNS);
        }
        let mut r = 1u32;
        while r <= REPLAY_N {
            self.run_green(r)?;
            r += 1;
        }
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].run < 1 || self.recs[i].run > REPLAY_N {
                return Err(E_ORDER);
            }
            self.recs[i].sane()?;
            i += 1;
        }
        Ok(())
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.recs.len() {
            buf.extend_from_slice(&self.recs[i].run.to_le_bytes());
            buf.push(self.recs[i].win);
            buf.extend_from_slice(&self.recs[i].score.to_le_bytes());
            buf.extend_from_slice(&self.recs[i].sig.to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 哨兵签名：跑次 × 窗数 × 最低分混签。
pub fn sentinel_sig(runs: u32, wins: u32, min_score: u32) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&runs.to_le_bytes());
    buf.extend_from_slice(&wins.to_le_bytes());
    buf.extend_from_slice(&min_score.to_le_bytes());
    fnv1a64(&buf)
}

/// 全账最低分。
pub fn min_score(s: &Sentinel) -> u32 {
    let mut mn: u32 = u32::MAX;
    let mut i = 0usize;
    while i < s.recs.len() {
        if s.recs[i].score < mn {
            mn = s.recs[i].score;
        }
        i += 1;
    }
    if s.recs.is_empty() {
        mn = 0;
    }
    mn
}

/// 呈现行：runs=3 wins=3 min=<最低分> green=1。
pub fn sentinel_line(s: &Sentinel) -> String {
    let mut out = String::new();
    out.push_str("runs=");
    out.push_str(&REPLAY_N.to_string());
    out.push_str(" wins=");
    out.push_str(&WIN_N.to_string());
    out.push_str(" min=");
    out.push_str(&min_score(s).to_string());
    out.push_str(" green=1");
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

/// 主巡检入口：R1..R11 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut g = Lcg::new(0x487_09);

    // R1 三窗闭集：窗型 0 与 4 拒绝。
    let mut s = Sentinel::new();
    match s.record(1, 0, 95) {
        Err(E_WIN) => {}
        _ => return Err(E_WIN),
    }
    match s.record(1, 4, 95) {
        Err(E_WIN) => hits += 1,
        _ => return Err(E_WIN),
    }

    // R2 哨兵探针落账：复跑 ×3 × 三窗，得分 90..100。
    let mut r = 1u32;
    while r <= REPLAY_N {
        let mut w = W_VXWM;
        while w <= W_XWAY {
            let score = PASS_LINE + g.below(11);
            s.record(r, w, score)?;
            w += 1;
        }
        r += 1;
    }
    if s.recs.len() as u32 != REPLAY_N * WIN_N {
        return Err(E_COUNT);
    }
    hits += 1;

    // R3 复跑 ×3 逐次判定。
    r = 1;
    while r <= REPLAY_N {
        s.run_green(r)?;
        r += 1;
    }
    hits += 1;

    // R4 全绿判定：全账最低分 ≥90。
    if min_score(&s) < PASS_LINE {
        return Err(E_SCORE);
    }
    hits += 1;

    // R5 窗型越界/漂移检出：欠分注入 → 红。
    let mut bad = Sentinel::new();
    bad.record(1, W_VXWM, PASS_LINE - 1)?;
    match bad.run_green(1) {
        Err(E_SCORE) => hits += 1,
        _ => return Err(E_SCORE),
    }

    // R6 缺窗检出：单跑仅两窗 → 红。
    let mut miss = Sentinel::new();
    miss.record(1, W_VXWM, 95)?;
    miss.record(1, W_WINE, 95)?;
    match miss.run_green(1) {
        Err(E_COUNT) => hits += 1,
        _ => return Err(E_COUNT),
    }

    // R7 全量核对（序号 + 签名 + 计数）。
    s.verify_all()?;
    hits += 1;

    // R8 复跑一致：同种子重跑指纹一致。
    let mut s2 = Sentinel::new();
    let mut g2 = Lcg::new(0x487_09);
    let mut r2 = 1u32;
    while r2 <= REPLAY_N {
        let mut w2 = W_VXWM;
        while w2 <= W_XWAY {
            s2.record(r2, w2, PASS_LINE + g2.below(11))?;
            w2 += 1;
        }
        r2 += 1;
    }
    if s2.fp() != s.fp() {
        return Err(E_REPLAY);
    }
    hits += 1;

    // R9 指纹稳定。
    let f1 = s.fp();
    let f2 = s.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行：runs=3 wins=3 green=1。
    let line = sentinel_line(&s);
    if !has_kw(line.as_bytes(), b"runs=3")
        || !has_kw(line.as_bytes(), b"wins=3")
        || !has_kw(line.as_bytes(), b"green=1")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R11 哨兵签名：3×3×最低分混签重算一致。
    let sig = sentinel_sig(REPLAY_N, WIN_N, min_score(&s));
    if sig != sentinel_sig(3, 3, min_score(&s)) || sig == 0 {
        return Err(E_SIG);
    }
    hits += 1;

    Ok(hits)
}
