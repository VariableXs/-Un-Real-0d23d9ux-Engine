//! UNX-F48515 —— E26 异常呈现三要素（限流/降级用户可见）
//! 判据原文：UNX-F48515-J1 E26 异常呈现固定三要素：
//! 一、限流发生必须可呈现（限流提示位）；二、降级发生必须可呈现
//! （降级提示位）；三、两者均须用户可见（可见性位）。
//! 任一要素缺失即异常呈现失败；要素文案非空、呈现账守恒、
//! 指纹与呈现行可复核、未触发异常时不得误呈现。
//! 判据落地（drill 场景）：R1 三要素闭集；R2 限流提示位；
//! R3 降级提示位；R4 用户可见位；R5 三要素齐全判定；
//! R6 未触发不误呈现；R7 文案闭集非空；R8 呈现账守恒；
//! R9 指纹稳定；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xEBA0;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_ELEM: u16 = SEG_BASE + 1; // 要素越出三要素闭集
const E_THR: u16 = SEG_BASE + 2; // 限流提示位缺失
const E_DEG: u16 = SEG_BASE + 3; // 降级提示位缺失
const E_VIS: u16 = SEG_BASE + 4; // 用户可见位缺失
const E_INCOMP: u16 = SEG_BASE + 5; // 三要素不齐仍判通过
const E_FALSE: u16 = SEG_BASE + 6; // 未触发误呈现
const E_TEXT: u16 = SEG_BASE + 7; // 文案闭集异常
const E_CONSERV: u16 = SEG_BASE + 8; // 呈现账不守恒
const E_FP: u16 = SEG_BASE + 9; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// 异常类别闭集：1 限流 / 2 降级。
pub const C_THROTTLE: u8 = 1;
pub const C_DEGRADE: u8 = 2;
/// 类别名（下标 = 类别-1）。
pub const CAUSE_NAMES: [&str; 2] = ["throttle", "degrade"];

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

/// 一笔异常呈现：类别 + 三要素位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Present {
    pub cause: u8,
    pub thr_bit: bool,
    pub deg_bit: bool,
    pub vis_bit: bool,
}

/// 三要素齐全判定：类别对应提示位 + 可见位必须同置。
pub fn complete(p: &Present) -> bool {
    let cause_bit = match p.cause {
        C_THROTTLE => p.thr_bit,
        C_DEGRADE => p.deg_bit,
        _ => false,
    };
    cause_bit && p.vis_bit
}

/// 文案闭集：两类别各一条非空文案。
pub fn cause_text(cause: u8) -> Option<&'static str> {
    match cause {
        C_THROTTLE => Some("rate limited: keysym reinjection throttled"),
        C_DEGRADE => Some("degraded: storm defense in reduced mode"),
        _ => None,
    }
}

/// 呈现账：入账笔数 + 可见笔数 + 类别计数。
pub struct PresBook {
    pub total: u32,
    pub visible: u32,
    pub thr_count: u32,
    pub deg_count: u32,
}

impl PresBook {
    pub fn new() -> PresBook {
        PresBook {
            total: 0,
            visible: 0,
            thr_count: 0,
            deg_count: 0,
        }
    }

    /// 入账一笔：类别闭集、文案非空、三要素齐全才可入账；
    /// 可见位与 visible 计数联动。
    pub fn put(&mut self, p: Present) -> Result<(), u16> {
        if p.cause != C_THROTTLE && p.cause != C_DEGRADE {
            return Err(E_ELEM);
        }
        if !complete(&p) {
            return Err(E_INCOMP);
        }
        match cause_text(p.cause) {
            Some(t) => {
                if t.is_empty() {
                    return Err(E_TEXT);
                }
            }
            None => return Err(E_ELEM),
        }
        self.total += 1;
        if p.vis_bit {
            self.visible += 1;
        }
        if p.cause == C_THROTTLE {
            self.thr_count += 1;
        } else {
            self.deg_count += 1;
        }
        Ok(())
    }

    /// 守恒：total == thr_count + deg_count；visible ≤ total。
    pub fn conserved(&self) -> bool {
        self.total == self.thr_count + self.deg_count && self.visible <= self.total
    }
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 三要素闭集：complete() 对三要素组合的判定自洽。
    let base = Present { cause: C_THROTTLE, thr_bit: false, deg_bit: false, vis_bit: false };
    if complete(&base) {
        return Err(E_ELEM);
    }
    let only_vis = Present { cause: C_THROTTLE, thr_bit: false, deg_bit: false, vis_bit: true };
    if complete(&only_vis) {
        return Err(E_ELEM);
    }
    let full = Present { cause: C_THROTTLE, thr_bit: true, deg_bit: false, vis_bit: true };
    if !complete(&full) {
        return Err(E_ELEM);
    }
    hits += 1;

    // R2 限流提示位：限流类别 thr_bit 缺失被拒，码恰为 E_INCOMP。
    let mut bk = PresBook::new();
    let bad_thr = Present { cause: C_THROTTLE, thr_bit: false, deg_bit: false, vis_bit: true };
    match bk.put(bad_thr) {
        Err(E_INCOMP) => hits += 1,
        _ => return Err(E_THR),
    }

    // R3 降级提示位：降级类别 deg_bit 缺失被拒，码恰为 E_INCOMP。
    let bad_deg = Present { cause: C_DEGRADE, thr_bit: false, deg_bit: false, vis_bit: true };
    match bk.put(bad_deg) {
        Err(E_INCOMP) => hits += 1,
        _ => return Err(E_DEG),
    }

    // R4 用户可见位：可见位缺失被拒，码恰为 E_INCOMP。
    let bad_vis = Present { cause: C_DEGRADE, thr_bit: false, deg_bit: true, vis_bit: false };
    match bk.put(bad_vis) {
        Err(E_INCOMP) => hits += 1,
        _ => return Err(E_VIS),
    }

    // R5 三要素齐全判定：两类齐全样本入账成功且计数分流正确。
    let ok_thr = Present { cause: C_THROTTLE, thr_bit: true, deg_bit: false, vis_bit: true };
    let ok_deg = Present { cause: C_DEGRADE, thr_bit: false, deg_bit: true, vis_bit: true };
    bk.put(ok_thr)?;
    bk.put(ok_deg)?;
    if bk.thr_count != 1 || bk.deg_count != 1 || bk.visible != 2 {
        return Err(E_INCOMP);
    }
    hits += 1;

    // R6 未触发不误呈现：空账 total=0、visible=0、守恒成立。
    let empty = PresBook::new();
    if empty.total != 0 || empty.visible != 0 || !empty.conserved() {
        return Err(E_FALSE);
    }
    hits += 1;

    // R7 文案闭集非空：两类别文案非空且互异；未知类别文案 None。
    let t1 = match cause_text(C_THROTTLE) {
        Some(t) => t,
        None => return Err(E_TEXT),
    };
    let t2 = match cause_text(C_DEGRADE) {
        Some(t) => t,
        None => return Err(E_TEXT),
    };
    if t1.is_empty() || t2.is_empty() || t1 == t2 {
        return Err(E_TEXT);
    }
    if cause_text(0).is_some() || cause_text(9).is_some() {
        return Err(E_TEXT);
    }
    hits += 1;

    // R8 呈现账守恒：LCG 混合 64 笔齐全样本，守恒与分流复核一致。
    let mut bk2 = PresBook::new();
    let mut lcg = Lcg::new(0x48515);
    let mut t = 0u32;
    while t < 64 {
        let cause = if lcg.below(2) == 0 { C_THROTTLE } else { C_DEGRADE };
        let p = Present {
            cause,
            thr_bit: cause == C_THROTTLE,
            deg_bit: cause == C_DEGRADE,
            vis_bit: true,
        };
        bk2.put(p)?;
        t += 1;
    }
    if !bk2.conserved() || bk2.total != 64 || bk2.visible != 64 {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R9 指纹稳定：同账两次指纹一致且非零。
    let f1 = book_fp(&bk2);
    let f2 = book_fp(&bk2);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行齐全：total=/vis=/thr=/deg= 四段前缀在位。
    let line = book_line(&bk2);
    if !has_kw(line.as_bytes(), b"total=")
        || !has_kw(line.as_bytes(), b"vis=")
        || !has_kw(line.as_bytes(), b"thr=")
        || !has_kw(line.as_bytes(), b"deg=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

/// 账面指纹：四项计数串联。
pub fn book_fp(bk: &PresBook) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&bk.total.to_le_bytes());
    buf.extend_from_slice(&bk.visible.to_le_bytes());
    buf.extend_from_slice(&bk.thr_count.to_le_bytes());
    buf.extend_from_slice(&bk.deg_count.to_le_bytes());
    fnv1a64(&buf)
}

/// 呈现行：total=<笔数> vis=<可见> thr=<限流> deg=<降级>。
pub fn book_line(bk: &PresBook) -> String {
    let mut s = String::new();
    s.push_str("total=");
    s.push_str(&bk.total.to_string());
    s.push_str(" vis=");
    s.push_str(&bk.visible.to_string());
    s.push_str(" thr=");
    s.push_str(&bk.thr_count.to_string());
    s.push_str(" deg=");
    s.push_str(&bk.deg_count.to_string());
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
