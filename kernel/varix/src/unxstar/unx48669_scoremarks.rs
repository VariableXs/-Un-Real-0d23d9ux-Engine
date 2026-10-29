//! UNX-F48669 —— I34 三类窗口打分回归哨兵（三窗 ×10 轮源标记零混淆）
//! 判据原文：UNX-F48669-J1 三类窗口（vxwm 原生/Wine/Xwayland）回归判据：
//! 三窗 ×10 轮源标记零混淆；判据锚定 域内宪法加严项。
//! 判据落地（drill 场景）：R1 三窗登记；R2 ×10 轮；R3 源标记零混淆；
//! R4 混淆检出；R5 窗闭集；R6 轮次计数；R7 缺窗检出；R8 哨兵指纹；
//! R9 呈现行；R10 复跑一致。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：源标记 = 打分数据源窗口标记，与窗类别一一绑定
//! （vxwm→1、Wine→2、Xwayland→3）；混淆 = 标记与窗不符；
//! 三窗 ×10 轮全量登记，缺窗、缺轮、错标皆哨兵红。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0B00;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_WIN: u16 = SEG_BASE + 1; // 窗类别越界
const E_ROUND: u16 = SEG_BASE + 2; // 轮次不足 10
const E_CONFUSE: u16 = SEG_BASE + 3; // 源标记混淆
const E_MISS: u16 = SEG_BASE + 4; // 缺窗
const E_FP: u16 = SEG_BASE + 5; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 6; // 呈现行缺段
const E_REPLAY: u16 = SEG_BASE + 7; // 复跑不一致
const E_CONSERV: u16 = SEG_BASE + 8; // 计数不守恒

/// 窗类别闭集与源标记映射。
pub const W_VXWM: u8 = 1; // vxwm 原生 → 源标记 1
pub const W_WINE: u8 = 2; // Wine → 源标记 2
pub const W_XWAY: u8 = 3; // Xwayland → 源标记 3
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

/// 窗类别闭集判定。
pub fn win_ok(w: u8) -> bool {
    w == W_VXWM || w == W_WINE || w == W_XWAY
}

/// 一轮一笔打分源标记。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScoreMark {
    pub round: u32,
    pub win: u8,
    pub source: u8, // 数据源标记，正确时 == win
}

/// 源标记正确性：source 必须等于窗类别（零混淆）。
pub fn mark_sane(m: &ScoreMark) -> bool {
    win_ok(m.win) && m.source == m.win
}

/// 打分回归哨兵账。
pub struct MarkSentinel {
    pub marks: Vec<ScoreMark>,
}

impl MarkSentinel {
    pub fn new() -> MarkSentinel {
        MarkSentinel { marks: vec![] }
    }

    /// 登记一笔：窗闭集 + 源标记零混淆。
    pub fn record(&mut self, round: u32, win: u8) -> Result<(), u16> {
        if !win_ok(win) {
            return Err(E_WIN);
        }
        let m = ScoreMark { round, win, source: win };
        if !mark_sane(&m) {
            return Err(E_CONFUSE);
        }
        self.marks.push(m);
        Ok(())
    }

    /// 轮次计数：每窗轮数。
    pub fn rounds_of(&self, win: u8) -> u32 {
        let mut c: u32 = 0;
        let mut i = 0usize;
        while i < self.marks.len() {
            if self.marks[i].win == win {
                c += 1;
            }
            i += 1;
        }
        c
    }

    /// 全账零混淆。
    pub fn no_confusion(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.marks.len() {
            if !mark_sane(&self.marks[i]) {
                return Err(E_CONFUSE);
            }
            i += 1;
        }
        Ok(())
    }

    /// 三窗 ×rounds 齐。
    pub fn windows_full(&self, rounds: u32) -> Result<(), u16> {
        let mut w = 1u8;
        while w <= 3 {
            if self.rounds_of(w) != rounds {
                return Err(E_MISS);
            }
            w += 1;
        }
        Ok(())
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.marks.len() {
            buf.extend_from_slice(&self.marks[i].round.to_le_bytes());
            buf.push(self.marks[i].win);
            buf.push(self.marks[i].source);
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 跑一轮三窗：round 轮 vxwm/wine/xway 各一笔。
pub fn run_round(s: &mut MarkSentinel, round: u32) -> Result<(), u16> {
    s.record(round, W_VXWM)?;
    s.record(round, W_WINE)?;
    s.record(round, W_XWAY)?;
    Ok(())
}

/// 逐窗逐轮抽验：取指定 (round, win) 的源标记。
pub fn mark_of(s: &MarkSentinel, round: u32, win: u8) -> Option<u8> {
    let mut i = 0usize;
    while i < s.marks.len() {
        if s.marks[i].round == round && s.marks[i].win == win {
            return Some(s.marks[i].source);
        }
        i += 1;
    }
    None
}

/// 逐轮抽验 N 轮：抽 rounds 逐轮三窗源标记与窗类别全等。
pub fn spot_check(s: &MarkSentinel, rounds: u32) -> Result<(), u16> {
    let mut r = 1u32;
    while r <= rounds {
        let mut w = 1u8;
        while w <= 3 {
            let src = mark_of(s, r, w).ok_or(E_MISS)?;
            if src != w {
                return Err(E_CONFUSE);
            }
            w += 1;
        }
        r += 1;
    }
    Ok(())
}

/// 窗名表（呈现辅助）。
pub fn win_name(w: u8) -> &'static str {
    if w == W_VXWM {
        "vxwm"
    } else if w == W_WINE {
        "wine"
    } else {
        "xwayland"
    }
}

/// 呈现行：w1=<n> w2=<n> w3=<n> confuse=<0|1>。
pub fn sentinel_line(s: &MarkSentinel) -> String {
    let mut out = String::new();
    out.push_str("w1=");
    out.push_str(&s.rounds_of(W_VXWM).to_string());
    out.push_str(" w2=");
    out.push_str(&s.rounds_of(W_WINE).to_string());
    out.push_str(" w3=");
    out.push_str(&s.rounds_of(W_XWAY).to_string());
    match s.no_confusion() {
        Ok(()) => out.push_str(" confuse=0"),
        Err(_) => out.push_str(" confuse=1"),
    }
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
    let mut g = Lcg::new(0x486_69);

    // R1 三窗登记：单轮三笔全部通过零混淆判定。
    let mut probe = MarkSentinel::new();
    run_round(&mut probe, 1)?;
    probe.no_confusion()?;
    hits += 1;

    // R2 ×10 轮：全账 30 笔。
    let mut s = MarkSentinel::new();
    let mut r: u32 = 1;
    while r <= ROUND_N {
        run_round(&mut s, r)?;
        r += 1;
    }
    if s.marks.len() as u32 != ROUND_N * 3 {
        return Err(E_ROUND);
    }
    hits += 1;

    // R3 源标记零混淆：全账扫描通过。
    // 零混淆双闸：全账扫描 + 抽验 5 轮逐窗全等。
    s.no_confusion()?;
    spot_check(&s, 5)?;
    hits += 1;

    // R4 混淆检出：篡改 source → 判定红。
    let mut bad = MarkSentinel::new();
    run_round(&mut bad, 1)?;
    bad.marks[1].source = W_VXWM; // wine 窗被标成 vxwm 源
    match bad.no_confusion() {
        Err(E_CONFUSE) => hits += 1,
        _ => return Err(E_CONFUSE),
    }

    // R5 窗闭集：win=0 与 win=9 拒收。
    let mut probe2 = MarkSentinel::new();
    match probe2.record(1, 0) {
        Err(E_WIN) => {}
        _ => return Err(E_WIN),
    }
    match probe2.record(1, 9) {
        Err(E_WIN) => hits += 1,
        _ => return Err(E_WIN),
    }

    // R6 轮次计数：每窗恰 10，窗名表冻结比对。
    if s.rounds_of(W_VXWM) != ROUND_N || s.rounds_of(W_WINE) != ROUND_N
        || s.rounds_of(W_XWAY) != ROUND_N
    {
        return Err(E_CONSERV);
    }
    if win_name(W_VXWM) != "vxwm" || win_name(W_WINE) != "wine"
        || win_name(W_XWAY) != "xwayland"
    {
        return Err(E_WIN);
    }
    hits += 1;

    // R7 缺窗检出：只跑两窗 → windows_full 红。
    let mut thin = MarkSentinel::new();
    let mut r = 1u32;
    while r <= ROUND_N {
        thin.record(r, W_VXWM)?;
        thin.record(r, W_WINE)?;
        r += 1;
    }
    match thin.windows_full(ROUND_N) {
        Err(E_MISS) => hits += 1,
        _ => return Err(E_MISS),
    }

    // R8 哨兵指纹稳定。
    let f1 = s.fp();
    let f2 = s.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R9 呈现行：w1=10 w2=10 w3=10 confuse=0。
    let line = sentinel_line(&s);
    if !has_kw(line.as_bytes(), b"w1=10") || !has_kw(line.as_bytes(), b"w2=10")
        || !has_kw(line.as_bytes(), b"w3=10") || !has_kw(line.as_bytes(), b"confuse=0")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R10 复跑一致：重建哨兵指纹一致，并全量抽验 10 轮。
    let mut s2 = MarkSentinel::new();
    let mut r = 1u32;
    while r <= ROUND_N {
        run_round(&mut s2, r)?;
        r += 1;
    }
    if s2.fp() != f1 {
        return Err(E_REPLAY);
    }
    spot_check(&s2, ROUND_N)?;
    let _ = g.below(10);
    hits += 1;

    Ok(hits)
}
